use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};

use crate::cli::AgentArgs;
use crate::runtime::{self, RuntimeOutcome};
use crate::state::{self, AgentStatus};
use crate::{config, git_ops, prompt, time};

/// Rebase an agent branch onto origin/master and resolve conflicts with Codex.
pub fn run(args: &AgentArgs, repo_override: Option<&Path>) -> Result<()> {
    self::run_with_target(args, repo_override, "origin/master", true)
}

/// Rebase an agent branch onto another branch and resolve conflicts with Codex.
pub fn run_onto_branch(
    args: &AgentArgs,
    repo_override: Option<&Path>,
    base_branch: &str,
) -> Result<()> {
    self::run_with_target(args, repo_override, base_branch, false)
}

fn run_with_target(
    args: &AgentArgs,
    repo_override: Option<&Path>,
    base_branch: &str,
    fetch_master: bool,
) -> Result<()> {
    let paths = config::repo_paths(repo_override)?;
    let state_path = paths.llmc_dir.join("state.json");
    let mut state = state::load_state(&state_path)?;
    let now = time::unix_timestamp()?;

    let agent_id = state::resolve_agent_id(args.agent.as_deref(), &state)?;
    let Some(record) = state.agents.get_mut(&agent_id) else {
        return Err(anyhow::anyhow!("Unknown agent id: {agent_id}"));
    };
    record.status = AgentStatus::Rebasing;
    record.last_run_unix = now;
    let record = record.clone();
    state::save_state(&state_path, &state)?;
    println!("agent_id={agent_id}");

    git_ops::ensure_clean_worktree(&record.worktree_path)?;
    if fetch_master {
        git_ops::fetch_master(&paths.repo_root)?;
    }
    let rebase_status = git_ops::rebase_onto_branch(&record.worktree_path, base_branch)?;

    if !rebase_status.success() {
        let status = git_ops::status_porcelain(&record.worktree_path)?;
        let user_prompt = format!(
            "Resolve the rebase conflicts for agent {agent_id}.\\n\\nRebase target: {base_branch}\\n\\nCurrent git status --porcelain:\\n{status}\\n\\nFinish the rebase with `git rebase --continue`, then run `just check` and `just clippy`.",
            agent_id = record.agent_id
        );
        let full_prompt =
            prompt::wrap_prompt(&paths.repo_root, &record.worktree_path, &user_prompt);

        self::update_prompt(&state_path, &record.agent_id, &full_prompt)?;
        let outcome = runtime::run_runtime(
            record.runtime,
            &full_prompt,
            &paths.repo_root,
            &record.worktree_path,
            false,
            record.claude_config.clone(),
        );
        self::record_runtime_result(&state_path, &record.agent_id, &full_prompt, &outcome)?;

        let outcome = outcome?;
        anyhow::ensure!(
            outcome.status.success(),
            "Runtime exited with status {status:?}",
            status = outcome.status
        );

        if git_ops::rebase_in_progress(&record.worktree_path)? {
            let continue_status = git_ops::rebase_continue(&record.worktree_path)?;
            anyhow::ensure!(
                continue_status.success(),
                "git rebase --continue failed in {worktree:?}",
                worktree = record.worktree_path
            );
        }

        self::run_just(&record.worktree_path, "check")?;
        self::run_just(&record.worktree_path, "clippy")?;
    }

    self::update_status(&state_path, &record.agent_id, AgentStatus::NeedsReview)?;

    if !rebase_status.success() {
        self::print_agent_completed(&record.agent_id);
    }

    Ok(())
}

fn update_prompt(state_path: &Path, agent_id: &str, prompt: &str) -> Result<()> {
    let mut state = state::load_state(state_path)?;
    let Some(record) = state.agents.get_mut(agent_id) else {
        return Err(anyhow::anyhow!("Unknown agent id: {}", agent_id));
    };
    record.prompt = prompt.to_string();
    record.last_run_unix = time::unix_timestamp()?;
    state::save_state(state_path, &state)?;

    Ok(())
}

fn record_runtime_result(
    state_path: &Path,
    agent_id: &str,
    prompt: &str,
    outcome: &Result<RuntimeOutcome>,
) -> Result<()> {
    let mut state = state::load_state(state_path)?;
    let Some(record) = state.agents.get_mut(agent_id) else {
        return Err(anyhow::anyhow!("Unknown agent id: {}", agent_id));
    };
    record.last_run_unix = time::unix_timestamp()?;
    record.last_pid = outcome.as_ref().ok().and_then(|outcome| outcome.pid);
    record.prompt = prompt.to_string();
    record.status = match outcome.as_ref() {
        Ok(outcome) if outcome.status.success() => AgentStatus::Rebasing,
        Ok(_) => AgentStatus::Idle,
        Err(_) => AgentStatus::Idle,
    };
    state::save_state(state_path, &state)?;

    Ok(())
}

fn update_status(state_path: &Path, agent_id: &str, status: AgentStatus) -> Result<()> {
    let mut state = state::load_state(state_path)?;
    let Some(record) = state.agents.get_mut(agent_id) else {
        return Err(anyhow::anyhow!("Unknown agent id: {}", agent_id));
    };
    record.status = status;
    record.last_run_unix = time::unix_timestamp()?;
    state::save_state(state_path, &state)?;

    Ok(())
}

fn run_just(worktree: &Path, command: &str) -> Result<()> {
    let status = Command::new("just")
        .arg(command)
        .current_dir(worktree)
        .status()
        .with_context(|| format!("Failed to run just {command} in {worktree:?}"))?;

    anyhow::ensure!(status.success(), "just {command} failed in {worktree:?}");

    Ok(())
}

fn print_agent_completed(agent_id: &str) {
    println!("Agent {agent_id} task completed");
}

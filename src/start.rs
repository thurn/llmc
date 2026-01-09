use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::cli::StartArgs;
use crate::state::{self, AgentRecord, AgentStatus, ClaudeConfig, Runtime};
use crate::{config, git_ops, notify, nouns, prompt, runtime, time};

/// Prepare the initial agent record and state needed for llmc start.
pub fn run(args: &StartArgs, repo_override: Option<&Path>) -> Result<()> {
    let paths = config::repo_paths(repo_override)?;
    fs::create_dir_all(&paths.worktrees_dir).with_context(|| {
        format!(
            "Failed to create worktrees dir {worktrees_dir:?}",
            worktrees_dir = paths.worktrees_dir
        )
    })?;
    fs::create_dir_all(&paths.llmc_dir).with_context(|| {
        format!("Failed to create llmc dir {llmc_dir:?}", llmc_dir = paths.llmc_dir)
    })?;

    let state_path = paths.llmc_dir.join("state.json");
    let state = state::load_state(&state_path)?;

    let agent_id = match &args.agent {
        Some(agent) => agent.clone(),
        None => nouns::choose_agent_id(&state)?,
    };

    anyhow::ensure!(!state.agents.contains_key(&agent_id), "Agent id already exists: {agent_id}");

    let runtime = args.runtime.unwrap_or(Runtime::Codex);

    let worktree_path = paths.worktrees_dir.join(format!("agent-{agent_id}"));
    anyhow::ensure!(!worktree_path.exists(), "Worktree already exists: {worktree_path:?}");

    git_ops::sync_master_to_origin(&paths.repo_root)?;

    git_ops::worktree_add(&paths.repo_root, &worktree_path, &format!("agent/{agent_id}"))?;
    git_ops::ensure_clean_worktree(&worktree_path)?;
    git_ops::copy_tabula_xlsm(&paths.repo_root, &worktree_path)?;

    let user_prompt = prompt::assemble_user_prompt(
        args.prompt.as_deref(),
        &self::prompt_files(args),
        args.prompt_pool.as_deref(),
    )?;
    let full_prompt = prompt::wrap_prompt(&paths.repo_root, &worktree_path, &user_prompt);

    let claude_config = if runtime == Runtime::Claude {
        Some(ClaudeConfig {
            model: args.claude_model.clone(),
            no_thinking: args.claude_no_thinking,
            sandbox: args.claude_sandbox.clone(),
            skip_permissions: args.claude_skip_permissions,
            allowed_tools: args.claude_allowed_tools.clone(),
            mcp_config: args.claude_mcp_config.clone(),
            interactive: args.claude_interactive,
        })
    } else {
        None
    };

    let now = time::unix_timestamp()?;

    let record = AgentRecord {
        agent_id: agent_id.clone(),
        branch: format!("agent/{agent_id}"),
        worktree_path: worktree_path.clone(),
        runtime,
        prompt: full_prompt.clone(),
        user_prompt: user_prompt.clone(),
        created_at_unix: now,
        last_run_unix: now,
        status: AgentStatus::Running,
        last_pid: None,
        claude_config: claude_config.clone(),
    };

    let mut state = state::load_state(&state_path)?;
    state.agents.insert(agent_id.clone(), record);
    state::save_state(&state_path, &state)?;

    let is_interactive = claude_config.as_ref().is_some_and(|c| c.interactive);

    let outcome = runtime::run_runtime(
        runtime,
        &full_prompt,
        &paths.repo_root,
        &worktree_path,
        args.background,
        claude_config,
    );

    let mut state = state::load_state(&state_path)?;
    {
        let Some(record) = state.agents.get_mut(&agent_id) else {
            return Err(anyhow::anyhow!("Agent record missing for {agent_id}"));
        };
        record.last_run_unix = time::unix_timestamp()?;
        record.last_pid = outcome.as_ref().ok().and_then(|outcome| outcome.pid);
        record.status = match outcome.as_ref() {
            Ok(outcome) if outcome.status.success() => AgentStatus::NeedsReview,
            Ok(_) if is_interactive => AgentStatus::NeedsReview,
            Ok(_) => AgentStatus::Idle,
            Err(_) => AgentStatus::Idle,
        };
    }
    state::save_state(&state_path, &state)?;

    let outcome = outcome?;
    if !outcome.status.success() && !is_interactive {
        anyhow::bail!("Runtime exited with status {status:?}", status = outcome.status);
    }
    self::print_agent_completed(&agent_id);

    if !args.no_notify
        && let Err(err) =
            notify::send_notification("LLMC", &format!("Agent {agent_id} task completed"))
    {
        eprintln!("Warning: Failed to send notification: {err:#}");
    }

    Ok(())
}

fn print_agent_completed(agent_id: &str) {
    println!("Agent {agent_id} task completed");
}

fn prompt_files(args: &StartArgs) -> Vec<PathBuf> {
    let mut files = args.prompt_file.clone();
    if let Some(path) = &args.prompt_file_pos {
        files.push(path.clone());
    }
    files
}

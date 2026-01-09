use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use crate::cli::RejectArgs;
use crate::state::{self, AgentRecord, AgentStatus};
use crate::{config, git_ops, prompt, runtime, time};

/// Re-run an agent with reviewer notes and the current diff.
pub fn run(args: &RejectArgs, repo_override: Option<&Path>) -> Result<()> {
    let paths = config::repo_paths(repo_override)?;
    let state_path = paths.llmc_dir.join("state.json");
    let state = state::load_state(&state_path)?;
    let agent_id = state::resolve_reviewed_agent_id(args.agent.as_deref(), &state)?;
    let Some(record) = state.agents.get(&agent_id) else {
        return Err(anyhow::anyhow!("Unknown agent id: {agent_id}"));
    };
    println!("agent_id={agent_id}");

    let notes = self::collect_notes(args)?;
    let diff = git_ops::diff_master_agent(&record.worktree_path, &record.branch)?;
    let user_prompt = self::build_reject_prompt(args, record, &notes, &diff);
    let updated_prompt = prompt::wrap_prompt(&paths.repo_root, &record.worktree_path, &user_prompt);

    let mut state = state::load_state(&state_path)?;
    let (runtime, worktree_path, claude_config) = {
        let Some(record) = state.agents.get_mut(&agent_id) else {
            return Err(anyhow::anyhow!("Unknown agent id: {agent_id}"));
        };
        record.prompt = updated_prompt.clone();
        record.status = AgentStatus::Running;
        record.last_run_unix = time::unix_timestamp()?;
        (record.runtime, record.worktree_path.clone(), record.claude_config.clone())
    };
    state::save_state(&state_path, &state)?;

    let outcome = runtime::run_runtime(
        runtime,
        &updated_prompt,
        &paths.repo_root,
        &worktree_path,
        false,
        claude_config,
    );

    let mut state = state::load_state(&state_path)?;
    {
        let Some(record) = state.agents.get_mut(&agent_id) else {
            return Err(anyhow::anyhow!("Unknown agent id: {agent_id}"));
        };
        record.last_run_unix = time::unix_timestamp()?;
        record.last_pid = outcome.as_ref().ok().and_then(|outcome| outcome.pid);
        record.status = match outcome.as_ref() {
            Ok(outcome) if outcome.status.success() => AgentStatus::NeedsReview,
            Ok(_) => AgentStatus::Idle,
            Err(_) => AgentStatus::Idle,
        };
    }
    state::save_state(&state_path, &state)?;

    let outcome = outcome?;
    anyhow::ensure!(
        outcome.status.success(),
        "Runtime exited with status {status:?}",
        status = outcome.status
    );
    self::print_agent_completed(&agent_id);

    Ok(())
}

fn collect_notes(args: &RejectArgs) -> Result<String> {
    let mut notes = Vec::new();

    if let Some(note) = &args.notes {
        notes.push(note.clone());
    }

    if let Some(path) = &args.notes_file {
        let contents = fs::read_to_string(path)
            .with_context(|| format!("Failed to read notes file {path:?}"))?;
        notes.push(contents);
    }

    if let Some(path) = &args.notes_file_pos {
        let contents = fs::read_to_string(path)
            .with_context(|| format!("Failed to read notes file {path:?}"))?;
        notes.push(contents);
    }

    Ok(notes.join("\n\n"))
}

fn build_reject_prompt(args: &RejectArgs, record: &AgentRecord, notes: &str, diff: &str) -> String {
    let mut sections = Vec::new();

    sections.push(
        "IMPORTANT: PRIMARY TASK IS TO IMPLEMENT THE CODE REVIEW NOTES ONLY. EVERYTHING BELOW IS CONTEXT. DO NOT RESTART THE TASK OR CHANGE ANYTHING ELSE."
            .to_string(),
    );

    if !notes.trim().is_empty() {
        sections.push(format!("Code review notes:\n{notes}"));
    }

    if args.include_prompt {
        let original_prompt = self::record_user_prompt(record);
        if !original_prompt.trim().is_empty() {
            sections.push(format!("Original prompt:\n{original_prompt}"));
        }
    }

    sections.push(format!("Diff:\n{diff}"));

    sections.join("\n\n")
}

fn record_user_prompt(record: &AgentRecord) -> &str {
    if !record.user_prompt.trim().is_empty() {
        return record.user_prompt.as_str();
    }

    self::extract_user_prompt(&record.prompt)
}

fn extract_user_prompt(prompt: &str) -> &str {
    let Some((_, user_prompt)) = prompt.split_once("\n\n") else {
        return "";
    };

    user_prompt
}

fn print_agent_completed(agent_id: &str) {
    println!("Agent {agent_id} task completed");
}

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use crate::cli::ContinueArgs;
use crate::state::{self, AgentRecord, AgentStatus, ClaudeConfig, Runtime};
use crate::{config, git_ops, prompt, runtime, time};

/// Continue an agent's work from where it left off.
pub fn run(args: &ContinueArgs, repo_override: Option<&Path>) -> Result<()> {
    let paths = config::repo_paths(repo_override)?;
    let state_path = paths.llmc_dir.join("state.json");
    let state = state::load_state(&state_path)?;

    let agent_id = match args.agent.as_deref() {
        Some(agent) => agent.to_string(),
        None => {
            if state.agents.len() == 1 {
                state.agents.keys().next().unwrap().clone()
            } else {
                return Err(anyhow::anyhow!(
                    "Multiple agents exist; pass --agent to specify which one to continue"
                ));
            }
        }
    };

    let Some(record) = state.agents.get(&agent_id) else {
        return Err(anyhow::anyhow!("Unknown agent id: {agent_id}"));
    };
    println!("agent_id={agent_id}");

    let notes = self::collect_notes(args)?;
    let git_status = git_ops::get_git_status(&record.worktree_path)?;
    let user_prompt = self::build_continue_prompt(record, &notes, &git_status);
    let updated_prompt = prompt::wrap_prompt(&paths.repo_root, &record.worktree_path, &user_prompt);

    let mut state = state::load_state(&state_path)?;
    let (runtime, worktree_path, claude_config) = {
        let Some(record) = state.agents.get_mut(&agent_id) else {
            return Err(anyhow::anyhow!("Unknown agent id: {agent_id}"));
        };
        record.prompt = updated_prompt.clone();
        record.status = AgentStatus::Running;
        record.last_run_unix = time::unix_timestamp()?;

        if let Some(new_runtime) = args.runtime {
            record.runtime = new_runtime;
        }

        let final_runtime = record.runtime;
        let claude_config = if final_runtime == Runtime::Claude {
            let existing_config = record.claude_config.as_ref();
            Some(ClaudeConfig {
                model: args
                    .claude_model
                    .clone()
                    .or_else(|| existing_config.and_then(|c| c.model.clone())),
                no_thinking: args.claude_no_thinking
                    || existing_config.is_some_and(|c| c.no_thinking),
                sandbox: args
                    .claude_sandbox
                    .clone()
                    .or_else(|| existing_config.and_then(|c| c.sandbox.clone())),
                skip_permissions: args.claude_skip_permissions
                    || existing_config.is_some_and(|c| c.skip_permissions),
                allowed_tools: args
                    .claude_allowed_tools
                    .clone()
                    .or_else(|| existing_config.and_then(|c| c.allowed_tools.clone())),
                mcp_config: if args.claude_mcp_config.is_empty() {
                    existing_config.map(|c| c.mcp_config.clone()).unwrap_or_default()
                } else {
                    args.claude_mcp_config.clone()
                },
                interactive: args.claude_interactive
                    || existing_config.is_some_and(|c| c.interactive),
            })
        } else {
            None
        };

        record.claude_config = claude_config.clone();

        (final_runtime, record.worktree_path.clone(), claude_config)
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

fn collect_notes(args: &ContinueArgs) -> Result<String> {
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

fn build_continue_prompt(record: &AgentRecord, notes: &str, git_status: &str) -> String {
    let mut sections = Vec::new();

    sections.push(
        "IMPORTANT: DO NOT RESTART THE TASK. Continue your work from where you left off. The task is already in progress and you should pick up from the current state. Use git status below to understand what has been done so far."
            .to_string(),
    );

    if !notes.trim().is_empty() {
        sections.push(format!("Additional context or instructions:\n{notes}"));
    }

    let original_prompt = self::record_user_prompt(record);
    if !original_prompt.trim().is_empty() {
        sections.push(format!("Original task prompt:\n{original_prompt}"));
    }

    sections.push(format!("Current git status:\n{git_status}"));

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

use std::path::Path;

use anyhow::{Context, Result};

use crate::cli::StatusArgs;
use crate::config;
use crate::state::{self, AgentStatus};

/// Display the current status of one or all agents.
pub fn run(args: &StatusArgs, repo_override: Option<&Path>) -> Result<()> {
    let paths = config::repo_paths(repo_override)?;
    let state_path = paths.llmc_dir.join("state.json");
    let state = state::load_state(&state_path)?;
    let Some(agent_id) = &args.agent else {
        for (index, record) in state.agents.values().enumerate() {
            if index > 0 {
                println!();
            }
            println!("agent_id={}", record.agent_id);
            println!("status={}", self::status_label(record.status));
        }
        return Ok(());
    };

    let record =
        state.agents.get(agent_id).with_context(|| format!("Unknown agent id: {agent_id}"))?;
    println!("agent_id={}", record.agent_id);
    println!("status={}", self::status_label(record.status));

    Ok(())
}

fn status_label(status: AgentStatus) -> &'static str {
    match status {
        AgentStatus::Idle => "idle",
        AgentStatus::Running => "running",
        AgentStatus::Rebasing => "rebasing",
        AgentStatus::NeedsReview => "needs_review",
        AgentStatus::Accepted => "accepted",
        AgentStatus::Rejected => "rejected",
    }
}

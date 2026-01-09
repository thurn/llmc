use std::path::Path;

use anyhow::{Context, Result};

use crate::cli::AgentArgs;
use crate::state::{self, AgentRecord};
use crate::{config, git_ops};

/// Delete an agent and force remove its worktree.
pub fn run(args: &AgentArgs, repo_override: Option<&Path>) -> Result<()> {
    let paths = config::repo_paths(repo_override)?;
    let state_path = paths.llmc_dir.join("state.json");
    let mut state = state::load_state(&state_path)?;
    let agent_id = state::resolve_agent_id(args.agent.as_deref(), &state)?;
    let Some(record) = state.agents.get(&agent_id) else {
        return Err(anyhow::anyhow!("Unknown agent id: {agent_id}"));
    };
    let record = record.clone();
    println!("agent_id={agent_id}");

    self::remove_agent(&paths.repo_root, &record)?;

    state.agents.remove(&agent_id);
    state::save_state(&state_path, &state)?;

    println!("Deleted agent {}", record.agent_id);

    Ok(())
}

fn remove_agent(repo_root: &Path, record: &AgentRecord) -> Result<()> {
    git_ops::worktree_remove_force(repo_root, &record.worktree_path)
        .with_context(|| format!("Failed to remove worktree for {}", record.agent_id))?;
    git_ops::branch_delete_force(repo_root, &record.branch)
        .with_context(|| format!("Failed to delete branch for {}", record.agent_id))?;

    Ok(())
}

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use crate::cli::CleanArgs;
use crate::state::{self, AgentRecord};
use crate::{config, git_ops};

/// Remove all agent state and delete all agent worktrees.
pub fn run(_args: &CleanArgs, repo_override: Option<&Path>) -> Result<()> {
    let paths = config::repo_paths(repo_override)?;
    let state_path = paths.llmc_dir.join("state.json");
    let state = state::load_state(&state_path)?;

    for record in state.agents.values() {
        self::remove_agent(&paths.repo_root, record)
            .with_context(|| format!("Failed to remove agent {}", record.agent_id))?;
    }

    if paths.worktrees_dir.exists() {
        fs::remove_dir_all(&paths.worktrees_dir).with_context(|| {
            format!(
                "Failed to remove worktrees directory {worktrees_dir:?}",
                worktrees_dir = paths.worktrees_dir
            )
        })?;
    }

    if paths.llmc_dir.exists() {
        fs::remove_dir_all(&paths.llmc_dir).with_context(|| {
            format!("Failed to remove llmc directory {llmc_dir:?}", llmc_dir = paths.llmc_dir)
        })?;
    }

    println!("Deleted all agent state");

    Ok(())
}

fn remove_agent(repo_root: &Path, record: &AgentRecord) -> Result<()> {
    git_ops::worktree_remove_force(repo_root, &record.worktree_path)
        .with_context(|| format!("Failed to remove worktree for {}", record.agent_id))?;
    git_ops::branch_delete_force(repo_root, &record.branch)
        .with_context(|| format!("Failed to delete branch for {}", record.agent_id))?;

    Ok(())
}

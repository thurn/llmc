use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::cli::{AcceptArgs, AgentArgs};
use crate::{config, git_ops, rebase, state};

/// Accept an agent branch by rebasing, fast-forwarding, and cleaning up.
pub fn run(args: &AcceptArgs, repo_override: Option<&Path>) -> Result<()> {
    let paths = config::repo_paths(repo_override)?;
    let state_path = paths.llmc_dir.join("state.json");
    let state = state::load_state(&state_path)?;
    let agent_id = state::resolve_reviewed_agent_id(args.agent.as_deref(), &state)?;
    let Some(record) = state.agents.get(&agent_id) else {
        return Err(anyhow::anyhow!("Unknown agent id: {agent_id}"));
    };
    println!("agent_id={agent_id}");

    git_ops::ensure_clean_worktree(&record.worktree_path)?;

    if !args.nopull {
        let source_root = self::source_repo_root(&paths.repo_root)?;
        git_ops::ensure_clean_worktree(&source_root)
            .context("Source repo must have a clean working tree before accept")?;
    }

    let range = format!("master..{}", record.branch);
    let ahead_count = git_ops::rev_list_count(&record.worktree_path, &range)?;
    anyhow::ensure!(
        ahead_count > 0,
        "Expected at least one commit ahead of master, found {ahead_count} for {range}"
    );

    rebase::run(&AgentArgs { agent: Some(agent_id.clone()) }, repo_override)?;

    git_ops::sync_master_to_origin(&paths.repo_root)?;
    if !git_ops::is_ancestor(&paths.repo_root, "master", &record.branch)? {
        rebase::run_onto_branch(
            &AgentArgs { agent: Some(agent_id.clone()) },
            repo_override,
            "master",
        )?;
    }

    let range = format!("master..{}", record.branch);
    let commit_count = git_ops::rev_list_count(&record.worktree_path, &range)?;
    if commit_count > 1 {
        let message = git_ops::oldest_commit_message(&record.worktree_path, &range)?;
        git_ops::reset_soft_to(&record.worktree_path, "master")?;
        git_ops::commit_with_message(&record.worktree_path, &message)?;
    }

    let message = git_ops::current_commit_message(&record.worktree_path)?;
    let cleaned_message = self::strip_agent_attribution(&message);
    if cleaned_message != message {
        git_ops::amend_commit_message(&record.worktree_path, &cleaned_message)?;
    }

    let commit = git_ops::rev_parse(&record.worktree_path, &record.branch)?;

    git_ops::checkout_master(&paths.repo_root)?;
    git_ops::merge_ff_only(&paths.repo_root, &record.branch)?;
    git_ops::worktree_remove(&paths.repo_root, &record.worktree_path)?;
    git_ops::branch_delete(&paths.repo_root, &record.branch)?;

    let mut state = state::load_state(&state_path)?;
    if state.agents.remove(&agent_id).is_none() {
        return Err(anyhow::anyhow!("Unknown agent id: {agent_id}"));
    }
    if state.last_reviewed_agent.as_deref() == Some(agent_id.as_str()) {
        state.last_reviewed_agent = None;
    }
    state::save_state(&state_path, &state)?;

    if !args.nopull {
        self::pull_to_source(&paths.repo_root, &agent_id, &commit)?;
    }

    Ok(())
}

/// Strip lines containing agent attribution markers from a commit message.
fn strip_agent_attribution(message: &str) -> String {
    message
        .lines()
        .filter(|line| !line.contains("Generated with") && !line.contains("Co-Authored-By"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn pull_to_source(llmc_root: &Path, agent_id: &str, commit: &str) -> Result<()> {
    let source_root = self::source_repo_root(llmc_root)?;
    git_ops::ensure_clean_worktree(&source_root)?;
    git_ops::fetch_from(&source_root, llmc_root, commit)?;

    let range = "master..FETCH_HEAD";
    let ahead_count = git_ops::rev_list_count(&source_root, range)?;
    anyhow::ensure!(
        ahead_count > 0,
        "Expected at least one commit ahead of master, found {ahead_count} for {range}"
    );

    let branch = format!("llmc/{agent_id}");
    git_ops::branch_force(&source_root, &branch, "FETCH_HEAD")?;
    git_ops::checkout_branch(&source_root, &branch)?;

    let rebase_status = git_ops::rebase_onto_branch(&source_root, "master")?;
    anyhow::ensure!(rebase_status.success(), "git rebase master failed in {source_root:?}");

    git_ops::checkout_master(&source_root)?;
    git_ops::merge_ff_only(&source_root, &branch)?;
    git_ops::branch_delete(&source_root, &branch)?;

    Ok(())
}

fn source_repo_root(llmc_root: &Path) -> Result<PathBuf> {
    let origin = git_ops::remote_origin_url(llmc_root)?;
    let trimmed = origin.trim();
    anyhow::ensure!(!trimmed.is_empty(), "remote.origin.url is empty in {llmc_root:?}");

    let path = trimmed.strip_prefix("file://").unwrap_or(trimmed);
    let source_root = PathBuf::from(path);
    let source_root =
        if source_root.is_absolute() { source_root } else { llmc_root.join(source_root) };
    let source_root = fs::canonicalize(&source_root)
        .with_context(|| format!("Failed to canonicalize source repo {source_root:?}"))?;

    anyhow::ensure!(
        source_root.join(".git").exists(),
        "remote.origin.url is not a local git repository: {origin}"
    );

    Ok(source_root)
}

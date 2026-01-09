use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

use anyhow::{Context, Result};

/// Ensure the git worktree has no pending changes.
pub fn ensure_clean_worktree(path: &Path) -> Result<()> {
    let status = self::status_porcelain(path)?;
    anyhow::ensure!(status.trim().is_empty(), "Working tree is not clean in {path:?}:\n{status}");

    Ok(())
}

/// Return `git status --porcelain` output.
pub fn status_porcelain(path: &Path) -> Result<String> {
    self::git_output(path, &["status", "--porcelain"])
}

/// Return `git status` output.
pub fn get_git_status(path: &Path) -> Result<String> {
    self::git_output(path, &["status"])
}

/// Return the oldest commit message for a revision range.
pub fn oldest_commit_message(repo_root: &Path, range: &str) -> Result<String> {
    let output =
        self::git_output(repo_root, &["log", "--reverse", "--format=%B", "-n", "1", range])?;

    Ok(output.trim_end().to_string())
}

/// Return the current HEAD commit message.
pub fn current_commit_message(repo_root: &Path) -> Result<String> {
    let output = self::git_output(repo_root, &["log", "--format=%B", "-n", "1", "HEAD"])?;

    Ok(output.trim_end().to_string())
}

/// Amend the current commit with a new message.
pub fn amend_commit_message(repo_root: &Path, message: &str) -> Result<()> {
    let mut child = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .arg("commit")
        .arg("--amend")
        .arg("--file")
        .arg("-")
        .stdin(Stdio::piped())
        .spawn()
        .with_context(|| format!("Failed to run git commit --amend in {repo_root:?}"))?;

    let message =
        if message.ends_with('\n') { message.to_string() } else { format!("{message}\n") };

    let Some(mut stdin) = child.stdin.take() else {
        return Err(anyhow::anyhow!("Failed to open git commit --amend stdin in {repo_root:?}"));
    };
    stdin
        .write_all(message.as_bytes())
        .with_context(|| format!("Failed to write amended commit message in {repo_root:?}"))?;
    drop(stdin);

    let status = child
        .wait()
        .with_context(|| format!("Failed to wait on git commit --amend in {repo_root:?}"))?;
    anyhow::ensure!(status.success(), "git commit --amend failed in {repo_root:?}");

    Ok(())
}

/// Soft reset the current branch to a revision.
pub fn reset_soft_to(repo_root: &Path, revision: &str) -> Result<()> {
    self::git_run(repo_root, &["reset", "--soft", revision])
}

/// Create a commit using a message passed via stdin.
pub fn commit_with_message(repo_root: &Path, message: &str) -> Result<()> {
    let mut child = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .arg("commit")
        .arg("--file")
        .arg("-")
        .stdin(Stdio::piped())
        .spawn()
        .with_context(|| format!("Failed to run git commit in {repo_root:?}"))?;

    let message =
        if message.ends_with('\n') { message.to_string() } else { format!("{message}\n") };

    let Some(mut stdin) = child.stdin.take() else {
        return Err(anyhow::anyhow!("Failed to open git commit stdin in {repo_root:?}"));
    };
    stdin
        .write_all(message.as_bytes())
        .with_context(|| format!("Failed to write commit message in {repo_root:?}"))?;
    drop(stdin);

    let status =
        child.wait().with_context(|| format!("Failed to wait on git commit in {repo_root:?}"))?;
    anyhow::ensure!(status.success(), "git commit failed in {repo_root:?}");

    Ok(())
}

/// Check if a revision is an ancestor of another.
pub fn is_ancestor(repo_root: &Path, ancestor: &str, descendant: &str) -> Result<bool> {
    let status = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .arg("merge-base")
        .arg("--is-ancestor")
        .arg(ancestor)
        .arg(descendant)
        .status()
        .with_context(|| {
            format!(
                "Failed to run git merge-base --is-ancestor {ancestor} {descendant} in {repo_root:?}"
            )
        })?;

    if status.success() {
        return Ok(true);
    }

    let Some(code) = status.code() else {
        anyhow::bail!("git merge-base --is-ancestor exited with signal in {repo_root:?}");
    };

    if code == 1 {
        return Ok(false);
    }

    anyhow::bail!("git merge-base --is-ancestor failed with status {status:?} in {repo_root:?}")
}

/// Update master to include origin/master.
pub fn sync_master_to_origin(repo_root: &Path) -> Result<()> {
    self::ensure_clean_worktree(repo_root)?;
    self::fetch_master(repo_root)?;
    self::checkout_master(repo_root)?;

    if self::is_ancestor(repo_root, "origin/master", "master")? {
        return Ok(());
    }

    if self::is_ancestor(repo_root, "master", "origin/master")? {
        return self::merge_ff_only(repo_root, "origin/master");
    }

    eprintln!(
        "Warning: master has diverged from origin/master in {repo_root:?}. Resetting to origin/master."
    );
    let status = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .arg("reset")
        .arg("--hard")
        .arg("origin/master")
        .status()
        .with_context(|| {
            format!("Failed to run git reset --hard origin/master in {repo_root:?}")
        })?;

    anyhow::ensure!(status.success(), "git reset --hard origin/master failed in {repo_root:?}");

    Ok(())
}

/// Create a new worktree for the agent branch.
pub fn worktree_add(repo_root: &Path, worktree_path: &Path, branch: &str) -> Result<()> {
    let status = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .arg("worktree")
        .arg("add")
        .arg("-b")
        .arg(branch)
        .arg(worktree_path)
        .arg("master")
        .status()
        .with_context(|| format!("Failed to add worktree {worktree_path:?}"))?;

    anyhow::ensure!(status.success(), "git worktree add failed for {worktree_path:?}");

    Ok(())
}

/// Copy gitignored Tabula.xlsm file from main repo to worktree.
pub fn copy_tabula_xlsm(repo_root: &Path, worktree_path: &Path) -> Result<()> {
    let source = repo_root.join("client/Assets/StreamingAssets/Tabula.xlsm");
    let dest = worktree_path.join("client/Assets/StreamingAssets/Tabula.xlsm");

    if !source.exists() {
        anyhow::bail!("Tabula.xlsm not found in main repo at {source:?}");
    }

    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory {parent:?}"))?;
    }

    std::fs::copy(&source, &dest)
        .with_context(|| format!("Failed to copy Tabula.xlsm from {source:?} to {dest:?}"))?;

    Ok(())
}

/// Remove the agent worktree from the repository.
pub fn worktree_remove(repo_root: &Path, worktree_path: &Path) -> Result<()> {
    let status = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .arg("worktree")
        .arg("remove")
        .arg(worktree_path)
        .status()
        .with_context(|| format!("Failed to remove worktree {worktree_path:?}"))?;

    anyhow::ensure!(status.success(), "git worktree remove failed for {worktree_path:?}");

    Ok(())
}

/// Force remove the agent worktree from the repository.
pub fn worktree_remove_force(repo_root: &Path, worktree_path: &Path) -> Result<()> {
    let status = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .arg("worktree")
        .arg("remove")
        .arg("--force")
        .arg(worktree_path)
        .status()
        .with_context(|| format!("Failed to remove worktree {worktree_path:?}"))?;

    anyhow::ensure!(status.success(), "git worktree remove failed for {worktree_path:?}");

    Ok(())
}

/// Delete the agent branch.
pub fn branch_delete(repo_root: &Path, branch: &str) -> Result<()> {
    self::git_run(repo_root, &["branch", "-d", branch])
}

/// Force delete the agent branch.
pub fn branch_delete_force(repo_root: &Path, branch: &str) -> Result<()> {
    self::git_run(repo_root, &["branch", "-D", branch])
}

/// Fetch the latest master branch.
pub fn fetch_master(repo_root: &Path) -> Result<()> {
    self::git_run(repo_root, &["fetch", "origin", "master"])
}

/// Fetch a revision from another local repo.
pub fn fetch_from(repo_root: &Path, source: &Path, revision: &str) -> Result<()> {
    let status = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .arg("fetch")
        .arg(source)
        .arg(revision)
        .status()
        .with_context(|| format!("Failed to fetch {revision} from {source:?} in {repo_root:?}"))?;

    anyhow::ensure!(status.success(), "git fetch failed for {revision} in {repo_root:?}");

    Ok(())
}

/// Start a rebase of the current branch onto another branch.
pub fn rebase_onto_branch(worktree_path: &Path, branch: &str) -> Result<ExitStatus> {
    self::git_status(worktree_path, &["rebase", branch])
}

/// Continue an in-progress rebase.
pub fn rebase_continue(worktree_path: &Path) -> Result<ExitStatus> {
    self::git_status(worktree_path, &["rebase", "--continue"])
}

/// Check if a rebase is in progress.
pub fn rebase_in_progress(worktree_path: &Path) -> Result<bool> {
    let rebase_merge =
        self::git_output(worktree_path, &["rev-parse", "--git-path", "rebase-merge"])?;
    let rebase_apply =
        self::git_output(worktree_path, &["rev-parse", "--git-path", "rebase-apply"])?;

    let merge_path = self::resolve_git_path(worktree_path, rebase_merge.trim());
    let apply_path = self::resolve_git_path(worktree_path, rebase_apply.trim());

    Ok(merge_path.exists() || apply_path.exists())
}

/// Return `git diff master...<branch>` output.
pub fn diff_master_agent(worktree_path: &Path, branch: &str) -> Result<String> {
    let range = format!("master...{branch}");
    self::git_output(worktree_path, &["diff", &range])
}

/// Run difftastic for `git diff master...<branch>`.
pub fn diff_master_agent_difftastic(worktree_path: &Path, branch: &str) -> Result<()> {
    let range = format!("master...{branch}");
    let status = Command::new("git")
        .arg("-C")
        .arg(worktree_path)
        .arg("-c")
        .arg("diff.external=difft")
        .arg("diff")
        .arg(&range)
        .status()
        .with_context(|| format!("Failed to diff {range} in {worktree_path:?}"))?;

    anyhow::ensure!(status.success(), "git diff failed for {range} in {worktree_path:?}");

    Ok(())
}

/// Resolve a revision to a commit hash.
pub fn rev_parse(repo_root: &Path, revision: &str) -> Result<String> {
    Ok(self::git_output(repo_root, &["rev-parse", revision])?.trim().to_string())
}

/// Return `git diff` output for unstaged changes.
pub fn diff_worktree(worktree_path: &Path) -> Result<String> {
    self::git_output(worktree_path, &["diff"])
}

/// Run difftastic for unstaged changes.
pub fn diff_worktree_difftastic(worktree_path: &Path) -> Result<()> {
    let status = Command::new("git")
        .arg("-C")
        .arg(worktree_path)
        .arg("-c")
        .arg("diff.external=difft")
        .arg("diff")
        .status()
        .with_context(|| format!("Failed to diff worktree {worktree_path:?}"))?;

    anyhow::ensure!(status.success(), "git diff failed for worktree {worktree_path:?}");

    Ok(())
}

/// Return `git diff --cached` output for staged changes.
pub fn diff_cached(worktree_path: &Path) -> Result<String> {
    self::git_output(worktree_path, &["diff", "--cached"])
}

/// Run difftastic for staged changes.
pub fn diff_cached_difftastic(worktree_path: &Path) -> Result<()> {
    let status = Command::new("git")
        .arg("-C")
        .arg(worktree_path)
        .arg("-c")
        .arg("diff.external=difft")
        .arg("diff")
        .arg("--cached")
        .status()
        .with_context(|| format!("Failed to diff cached changes in {worktree_path:?}"))?;

    anyhow::ensure!(status.success(), "git diff --cached failed for {worktree_path:?}");

    Ok(())
}

/// Return the number of commits for a revision range.
pub fn rev_list_count(worktree_path: &Path, range: &str) -> Result<usize> {
    let output = self::git_output(worktree_path, &["rev-list", "--count", range])?;
    let trimmed = output.trim();
    let count: usize =
        trimmed.parse().with_context(|| format!("Failed to parse rev-list count {trimmed:?}"))?;

    Ok(count)
}

/// Checkout the master branch at the repository root.
pub fn checkout_master(repo_root: &Path) -> Result<()> {
    self::git_run(repo_root, &["checkout", "master"])
}

/// Checkout a branch at the repository root.
pub fn checkout_branch(repo_root: &Path, branch: &str) -> Result<()> {
    self::git_run(repo_root, &["checkout", branch])
}

/// Fast-forward merge the agent branch into master.
pub fn merge_ff_only(repo_root: &Path, branch: &str) -> Result<()> {
    self::git_run(repo_root, &["merge", "--ff-only", branch])
}

/// Create or update a branch to a revision.
pub fn branch_force(repo_root: &Path, branch: &str, revision: &str) -> Result<()> {
    self::git_run(repo_root, &["branch", "-f", branch, revision])
}

/// Read the configured origin URL.
pub fn remote_origin_url(repo_root: &Path) -> Result<String> {
    Ok(self::git_output(repo_root, &["config", "--get", "remote.origin.url"])?.trim().to_string())
}

fn git_run(repo_root: &Path, args: &[&str]) -> Result<()> {
    let status = self::git_status(repo_root, args)?;
    anyhow::ensure!(status.success(), "git command failed: git -C {repo_root:?} {args:?}");

    Ok(())
}

fn git_status(repo_root: &Path, args: &[&str]) -> Result<ExitStatus> {
    Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(args)
        .status()
        .with_context(|| format!("Failed to run git {args:?} in {repo_root:?}"))
}

fn git_output(repo_root: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(args)
        .output()
        .with_context(|| format!("Failed to run git {args:?} in {repo_root:?}"))?;

    anyhow::ensure!(output.status.success(), "git command failed: git -C {repo_root:?} {args:?}");

    String::from_utf8(output.stdout)
        .with_context(|| format!("git output was not UTF-8 for git -C {repo_root:?} {args:?}"))
}

fn resolve_git_path(worktree_path: &Path, git_path: &str) -> PathBuf {
    let path = Path::new(git_path);
    if path.is_absolute() { path.to_path_buf() } else { worktree_path.join(path) }
}

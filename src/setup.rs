use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};

use crate::cli::SetupArgs;
use crate::config;
use crate::state::{self, StateFile};

const REQUIRED_BINARIES: &[&str] = &[
    "git",
    "git-lfs",
    "claude",
    "codex",
    "gemini",
    "cursor",
    "forgejo",
    "difft",
    "code",
    "osascript",
];

/// Perform llmc setup by cloning a repo and preparing LLMC directories.
pub fn run(args: &SetupArgs, repo_override: Option<&Path>) -> Result<()> {
    let source = match &args.source {
        Some(source) => source.clone(),
        None => config::repo_root(repo_override)?,
    };
    let target = match &args.target {
        Some(target) => target.clone(),
        None => config::default_target_dir()?,
    };

    self::ensure_dependencies()?;
    self::ensure_target_dir(&target)?;
    self::clone_repo(&source, &target)?;
    self::validate_gitignore(&target)?;
    self::configure_rerere(&target)?;
    self::configure_lfs(&target)?;
    self::create_dirs(&target)?;
    state::save_state(&target.join(".llmc").join("state.json"), &StateFile::default())?;

    Ok(())
}

fn ensure_dependencies() -> Result<()> {
    for binary in REQUIRED_BINARIES {
        self::ensure_binary(binary)?;
    }

    Ok(())
}

fn ensure_binary(binary: &str) -> Result<()> {
    let status = Command::new("which")
        .arg(binary)
        .status()
        .with_context(|| format!("Failed to check for {binary}"))?;

    anyhow::ensure!(status.success(), "Missing required dependency: {binary}");

    Ok(())
}

fn ensure_target_dir(target: &Path) -> Result<()> {
    if target.exists() {
        let mut entries =
            fs::read_dir(target).with_context(|| format!("Failed to read {target:?}"))?;
        anyhow::ensure!(entries.next().is_none(), "Target directory is not empty: {target:?}");
        return Ok(());
    }

    fs::create_dir_all(target)
        .with_context(|| format!("Failed to create target directory {target:?}"))
}

fn clone_repo(source: &Path, target: &Path) -> Result<()> {
    let status = Command::new("git")
        .arg("clone")
        .arg("--local")
        .arg(source)
        .arg(target)
        .status()
        .with_context(|| format!("Failed to clone {source:?} to {target:?}"))?;

    anyhow::ensure!(status.success(), "git clone failed for {source:?}");

    Ok(())
}

fn validate_gitignore(target: &Path) -> Result<()> {
    let gitignore_path = target.join(".gitignore");
    let contents = fs::read_to_string(&gitignore_path)
        .with_context(|| format!("Failed to read {gitignore_path:?}"))?;
    let has_entry = contents.lines().map(str::trim).any(|line| matches!(line, ".llmc" | ".llmc/"));

    anyhow::ensure!(has_entry, "Missing .llmc/ entry in {gitignore_path:?}");

    Ok(())
}

fn configure_rerere(target: &Path) -> Result<()> {
    self::run_git(target, &["config", "rerere.enabled", "true"])?;
    self::run_git(target, &["config", "rerere.autoupdate", "true"])
}

fn configure_lfs(target: &Path) -> Result<()> {
    self::run_git(target, &["lfs", "install"])?;
    self::run_git(target, &["lfs", "pull"])
}

fn create_dirs(target: &Path) -> Result<()> {
    let worktrees_dir = target.join(".worktrees");
    let llmc_dir = target.join(".llmc");

    fs::create_dir_all(&worktrees_dir)
        .with_context(|| format!("Failed to create {worktrees_dir:?}"))?;
    fs::create_dir_all(&llmc_dir).with_context(|| format!("Failed to create {llmc_dir:?}"))
}

fn run_git(target: &Path, args: &[&str]) -> Result<()> {
    let status = Command::new("git")
        .arg("-C")
        .arg(target)
        .args(args)
        .status()
        .with_context(|| format!("Failed to run git {args:?} in {target:?}"))?;

    anyhow::ensure!(status.success(), "git command failed: git -C {target:?} {args:?}");

    Ok(())
}

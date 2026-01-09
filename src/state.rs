use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use atomic_write_file::AtomicWriteFile;
use clap::ValueEnum;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentRecord {
    pub agent_id: String,
    pub branch: String,
    pub worktree_path: PathBuf,
    pub runtime: Runtime,
    pub prompt: String,
    #[serde(default)]
    pub user_prompt: String,
    pub created_at_unix: u64,
    pub last_run_unix: u64,
    pub status: AgentStatus,
    pub last_pid: Option<u32>,
    #[serde(default)]
    pub claude_config: Option<ClaudeConfig>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct StateFile {
    pub agents: BTreeMap<String, AgentRecord>,
    pub last_reviewed_agent: Option<String>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    Idle,
    Running,
    Rebasing,
    NeedsReview,
    Accepted,
    Rejected,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
#[clap(rename_all = "lowercase")]
pub enum Runtime {
    Claude,
    Codex,
    Gemini,
    Cursor,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ClaudeConfig {
    pub model: Option<String>,
    pub no_thinking: bool,
    pub sandbox: Option<String>,
    pub skip_permissions: bool,
    pub allowed_tools: Option<String>,
    pub mcp_config: Vec<String>,
    pub interactive: bool,
}

/// Return the oldest agent id in needs_review status.
pub fn oldest_pending_agent_id(state: &StateFile) -> Option<String> {
    state
        .agents
        .values()
        .filter(|record| record.status == AgentStatus::NeedsReview)
        .min_by_key(|record| record.created_at_unix)
        .map(|record| record.agent_id.clone())
}

/// Return the oldest agent id
pub fn oldest_agent_id(state: &StateFile) -> Option<String> {
    state
        .agents
        .values()
        .min_by_key(|record| record.created_at_unix)
        .map(|record| record.agent_id.clone())
}

/// Resolve an agent id or select the oldest pending agent.
pub fn resolve_agent_id(agent: Option<&str>, state: &StateFile) -> Result<String> {
    let Some(agent) = agent else {
        let Some(agent_id) = oldest_pending_agent_id(state) else {
            return Err(anyhow::anyhow!("No agents in needs_review state"));
        };
        return Ok(agent_id);
    };

    Ok(agent.to_string())
}

/// Resolve an agent id or use the most recently reviewed agent.
pub fn resolve_reviewed_agent_id(agent: Option<&str>, state: &StateFile) -> Result<String> {
    let Some(agent) = agent else {
        let Some(agent_id) = state.last_reviewed_agent.as_deref() else {
            return Err(anyhow::anyhow!(
                "No last reviewed agent found; run llmc review or pass --agent"
            ));
        };
        return Ok(agent_id.to_string());
    };

    Ok(agent.to_string())
}

/// Load the LLMC state file if present, otherwise return a default state.
pub fn load_state(path: &Path) -> Result<StateFile> {
    if !path.exists() {
        return Ok(StateFile::default());
    }

    let data = fs::read(path).with_context(|| format!("Failed to read state file {path:?}"))?;
    serde_json::from_slice(&data).with_context(|| format!("Failed to parse state file {path:?}"))
}

/// Write the LLMC state file atomically to disk.
pub fn save_state(path: &Path, state: &StateFile) -> Result<()> {
    let Some(parent) = path.parent() else {
        return Err(anyhow::anyhow!("State path has no parent: {path:?}"));
    };

    fs::create_dir_all(parent)
        .with_context(|| format!("Failed to create state directory {parent:?}"))?;

    let data = serde_json::to_vec_pretty(state)
        .with_context(|| format!("Failed to serialize state file {path:?}"))?;

    let mut file = AtomicWriteFile::options()
        .open(path)
        .with_context(|| format!("Failed to open state file {path:?}"))?;
    file.write_all(&data).with_context(|| format!("Failed to write state file {path:?}"))?;
    file.commit().with_context(|| format!("Failed to commit state file {path:?}"))?;

    Ok(())
}

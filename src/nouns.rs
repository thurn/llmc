use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::state::StateFile;

/// Select a short noun that is not already used as an agent id.
pub fn choose_agent_id(state: &StateFile) -> Result<String> {
    let mut options = self::load_nouns()?;
    fastrand::shuffle(&mut options);

    options
        .into_iter()
        .find(|noun| !state.agents.contains_key(noun))
        .ok_or_else(|| anyhow::anyhow!("No unused agent ids available"))
}

fn load_nouns() -> Result<Vec<String>> {
    let path = self::nouns_path();
    let contents =
        fs::read_to_string(&path).with_context(|| format!("Failed to read nouns file {path:?}"))?;
    let nouns: Vec<String> =
        contents.lines().map(str::trim).filter(|line| !line.is_empty()).map(String::from).collect();

    anyhow::ensure!(!nouns.is_empty(), "Nouns file is empty: {path:?}");

    Ok(nouns)
}

fn nouns_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("nouns.txt")
}

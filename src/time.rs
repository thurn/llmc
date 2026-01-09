use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};

/// Return the current Unix timestamp in seconds.
pub fn unix_timestamp() -> Result<u64> {
    let since_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .with_context(|| "System time was before the Unix epoch")?;

    Ok(since_epoch.as_secs())
}

//! Built-in Vosk model embedded at compile time.
//!
//! Vosk's C API requires a filesystem path, so the embedded model is extracted
//! once to a per-user cache directory and that path is returned.

use include_dir::{include_dir, Dir};
use std::path::PathBuf;

static EMBEDDED_MODEL: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/model");

const MODEL_ID: &str = "vosk-model-small-en-us-0.15";

/// Ensure the embedded model is extracted and return its directory path.
pub fn ensure_extracted() -> Result<PathBuf, String> {
    let base = cache_base_dir()?;
    let target = base.join(MODEL_ID);
    let marker = target.join(".cuepal-extracted");

    if marker.exists() && target.is_dir() {
        return Ok(target);
    }

    // Clean partial/old extraction
    if target.exists() {
        let _ = std::fs::remove_dir_all(&target);
    }
    std::fs::create_dir_all(&target)
        .map_err(|e| format!("create model cache dir {}: {e}", target.display()))?;

    EMBEDDED_MODEL
        .extract(&target)
        .map_err(|e| format!("extract embedded model to {}: {e}", target.display()))?;

    std::fs::write(&marker, MODEL_ID.as_bytes())
        .map_err(|e| format!("write model marker {}: {e}", marker.display()))?;

    Ok(target)
}

fn cache_base_dir() -> Result<PathBuf, String> {
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        return Ok(PathBuf::from(local).join("CuePal").join("models"));
    }
    if let Some(home) = std::env::var_os("USERPROFILE") {
        return Ok(PathBuf::from(home).join(".cuepal").join("models"));
    }
    Err("no LOCALAPPDATA/USERPROFILE for model cache".to_string())
}

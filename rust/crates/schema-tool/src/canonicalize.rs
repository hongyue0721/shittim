use anyhow::{Context, Result};
use kernel_contracts::canonical::{canonical_json_bytes, sha256_hex};
use std::path::Path;

pub fn run(json_file: &Path, hash: bool) -> Result<()> {
    let text = std::fs::read_to_string(json_file)
        .with_context(|| format!("read {}", json_file.display()))?;
    let value: serde_json::Value =
        serde_json::from_str(&text).with_context(|| format!("parse {}", json_file.display()))?;
    let bytes = canonical_json_bytes(&value).map_err(|e| anyhow::anyhow!(e.to_string()))?;
    if hash {
        let digest = sha256_hex(&bytes);
        print!("{digest}");
    } else {
        // Write raw canonical bytes without extra newline for exact JCS output.
        use std::io::Write;
        let mut stdout = std::io::stdout().lock();
        stdout.write_all(&bytes)?;
        stdout.flush()?;
    }
    Ok(())
}

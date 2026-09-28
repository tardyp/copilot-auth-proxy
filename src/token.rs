use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::RngCore;
use std::fs;
use std::path::Path;

pub fn ensure_client_token(token_path: &Path) -> Result<String, String> {
    if let Ok(content) = fs::read_to_string(token_path) {
        let trimmed = content.trim().to_string();
        if !trimmed.is_empty() {
            return Ok(trimmed);
        }
    }

    if let Some(parent) = token_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create directory {}: {}", parent.display(), e))?;
    }

    let mut random_bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut random_bytes);
    let token = URL_SAFE_NO_PAD.encode(random_bytes);

    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(token_path)
            .map_err(|e| {
                format!(
                    "Failed to write api-token file {}: {}",
                    token_path.display(),
                    e
                )
            })?;
        file.write_all(token.as_bytes())
            .map_err(|e| format!("Failed to write api-token data: {}", e))?;
    }

    #[cfg(not(unix))]
    {
        fs::write(token_path, &token).map_err(|e| {
            format!(
                "Failed to write api-token file {}: {}",
                token_path.display(),
                e
            )
        })?;
    }

    Ok(token)
}

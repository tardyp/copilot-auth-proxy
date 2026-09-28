use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct ProxyPaths {
    #[allow(dead_code)]
    pub directory: PathBuf,
    pub auth_db: PathBuf,
    pub api_token: PathBuf,
}

pub fn resolve_proxy_paths() -> Result<ProxyPaths, String> {
    let home = dirs::home_dir().ok_or_else(|| "Could not determine home directory".to_string())?;
    let directory = home.join(".copilot-openai-proxy");
    Ok(ProxyPaths {
        auth_db: directory.join("auth.db"),
        api_token: directory.join("api-token"),
        directory,
    })
}

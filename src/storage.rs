use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CopilotOAuthData {
    pub access: String,
    #[serde(default)]
    pub refresh: String,
    #[serde(default)]
    pub expires: u64,
    #[serde(default, rename = "apiEndpoint")]
    pub api_endpoint: Option<String>,
    #[serde(default, rename = "enterpriseUrl")]
    pub enterprise_url: Option<String>,
    #[serde(default, rename = "authorizedAt")]
    pub authorized_at: Option<u64>,
}

pub struct AuthStorage {
    conn: Connection,
}

impl AuthStorage {
    pub fn open(db_path: &Path) -> Result<Self, String> {
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create directory {}: {}", parent.display(), e))?;
        }
        let conn = Connection::open(db_path).map_err(|e| {
            format!(
                "Failed to open SQLite database {}: {}",
                db_path.display(),
                e
            )
        })?;

        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS auth_credentials (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                provider TEXT NOT NULL,
                credential_type TEXT NOT NULL,
                data TEXT NOT NULL,
                disabled_cause TEXT DEFAULT NULL,
                identity_key TEXT DEFAULT NULL,
                created_at INTEGER NOT NULL DEFAULT (CAST(strftime('%s','now') AS INTEGER)),
                updated_at INTEGER NOT NULL DEFAULT (CAST(strftime('%s','now') AS INTEGER))
            );
            CREATE INDEX IF NOT EXISTS idx_auth_credentials_provider ON auth_credentials(provider);
            "#,
        )
        .map_err(|e| format!("Failed to initialize schema: {}", e))?;

        Ok(AuthStorage { conn })
    }

    pub fn get_copilot_credentials(&self) -> Result<Option<CopilotOAuthData>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT data FROM auth_credentials WHERE provider = 'github-copilot' AND disabled_cause IS NULL ORDER BY id DESC LIMIT 1")
            .map_err(|e| format!("Prepare query failed: {}", e))?;

        let mut rows = stmt.query([]).map_err(|e| format!("Query failed: {}", e))?;

        if let Some(row) = rows
            .next()
            .map_err(|e| format!("Fetch row failed: {}", e))?
        {
            let data_str: String = row
                .get(0)
                .map_err(|e| format!("Column read failed: {}", e))?;
            let data: CopilotOAuthData = serde_json::from_str(&data_str)
                .map_err(|e| format!("Failed to parse credential json: {}", e))?;
            Ok(Some(data))
        } else {
            Ok(None)
        }
    }
    pub fn save_copilot_credentials(&self, data: &CopilotOAuthData) -> Result<(), String> {
        let json_str = serde_json::to_string(data)
            .map_err(|e| format!("Failed to serialize credentials: {}", e))?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        // Check if row exists
        let exists: bool = self
            .conn
            .query_row(
                "SELECT 1 FROM auth_credentials WHERE provider = 'github-copilot' LIMIT 1",
                [],
                |_| Ok(true),
            )
            .unwrap_or(false);

        if exists {
            self.conn
                .execute(
                    "UPDATE auth_credentials SET data = ?1, updated_at = ?2, disabled_cause = NULL WHERE provider = 'github-copilot'",
                    params![json_str, now],
                )
                .map_err(|e| format!("Failed to update credentials: {}", e))?;
        } else {
            self.conn
                .execute(
                    "INSERT INTO auth_credentials (provider, credential_type, data, created_at, updated_at) VALUES ('github-copilot', 'oauth', ?1, ?2, ?2)",
                    params![json_str, now],
                )
                .map_err(|e| format!("Failed to insert credentials: {}", e))?;
        }

        Ok(())
    }
}

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub username: String,
    pub user_id: u64,
    pub bind_addr: String,
    pub title_id: u32,
}

impl Default for ServerConfig {
    fn default() -> Self {
        let username = whoami().unwrap_or_else(|| "Player".to_string());
        Self {
            username,
            user_id: 1337133713371337,
            bind_addr: "127.0.0.1".to_string(),
            title_id: 27551, // BOCW Steam title ID
        }
    }
}

impl ServerConfig {
    pub fn load_or_default() -> Self {
        let config_path = config_file_path();
        if config_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&config_path) {
                if let Ok(cfg) = toml::from_str::<ServerConfig>(&content) {
                    return cfg;
                }
            }
        }

        let cfg = Self::default();
        if let Ok(toml_str) = toml::to_string_pretty(&cfg) {
            let _ = std::fs::create_dir_all(config_path.parent().unwrap());
            let _ = std::fs::write(&config_path, toml_str);
        }
        cfg
    }
}

fn config_file_path() -> PathBuf {
    let mut path = std::env::current_exe().unwrap_or_default();
    path.pop();
    path.push("t9-server.toml");
    path
}

fn whoami() -> Option<String> {
    std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .ok()
}

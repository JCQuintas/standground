use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub auto_restore: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            auto_restore: true,
        }
    }
}

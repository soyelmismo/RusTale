use std::collections::HashMap;
use std::path::PathBuf;

pub struct ServerState {
    pub username: String,
    pub uuid: String,
    pub skins: HashMap<String, serde_json::Value>,
    pub game_dir: PathBuf,
    pub last_server_uuid: Option<String>,
}

impl ServerState {
    pub fn new(username: String, uuid: String, game_dir: PathBuf) -> Self {
        Self {
            username,
            uuid,
            skins: HashMap::new(),
            game_dir,
            last_server_uuid: None,
        }
    }
}

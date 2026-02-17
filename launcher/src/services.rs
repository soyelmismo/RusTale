use crate::game::patch_api::PatchApiFrontend;

#[derive(Clone)]
pub struct Services {
    pub patcher: PatchApiFrontend,
    pub api_client: reqwest::Client,
    pub download_client: reqwest::Client,
    // Future services: Auth, ServerManager
}

impl Services {
    pub fn new(api_client: reqwest::Client, download_client: reqwest::Client) -> Self {
        Self {
            patcher: PatchApiFrontend::new(),
            api_client,
            download_client,
        }
    }
}

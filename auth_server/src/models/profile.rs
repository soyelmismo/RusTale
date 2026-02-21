use serde::Serialize;

#[derive(Serialize)]
pub struct ProfileLookupResponse {
    pub uuid: String,
    pub username: String,
}

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub server: String,
}

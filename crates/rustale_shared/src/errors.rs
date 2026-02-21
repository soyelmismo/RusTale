use thiserror::Error;

#[derive(Error, Debug)]
pub enum EngineError {
    #[error("Profile not found: {0}")]
    ProfileNotFound(String),
    #[error("IO Error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Serialization Error: {0}")]
    Serialization(#[from] toml::ser::Error),
    #[error("Deserialization Error: {0}")]
    Deserialization(#[from] toml::de::Error),
    #[error("Unknown error: {0}")]
    Unknown(String),
}


use std::fmt::Display;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum CoreError {
    NetworkError(String),
    IOError(String),
    AuthError { 
        message: String, 
        can_retry: bool 
    },
    LaunchError(String),
    ModInstallError(String),
    Cancelled,
    GenericError(String),
}

impl Display for CoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CoreError::NetworkError(msg) => write!(f, "Network error: {}", msg),
            CoreError::IOError(msg) => write!(f, "IO error: {}", msg),
            CoreError::AuthError { message, .. } => write!(f, "Authentication error: {}", message),
            CoreError::LaunchError(msg) => write!(f, "Launch error: {}", msg),
            CoreError::ModInstallError(msg) => write!(f, "Mod installation error: {}", msg),
            CoreError::Cancelled => write!(f, "Operation cancelled"),
            CoreError::GenericError(msg) => write!(f, "Error: {}", msg),
        }
    }
}

impl From<anyhow::Error> for CoreError {
    fn from(err: anyhow::Error) -> Self {
        CoreError::GenericError(err.to_string())
    }
}

impl From<std::io::Error> for CoreError {
    fn from(err: std::io::Error) -> Self {
        CoreError::IOError(err.to_string())
    }
}

impl From<rustale_shared::reqwest::Error> for CoreError {
    fn from(err: rustale_shared::reqwest::Error) -> Self {
        CoreError::NetworkError(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_anyhow_error() {
        let anyhow_err = anyhow::anyhow!("test error");
        let core_err: CoreError = anyhow_err.into();
        
        match core_err {
            CoreError::GenericError(msg) => assert!(msg.contains("test error")),
            _ => panic!("Expected GenericError variant"),
        }
    }

    #[test]
    fn test_from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
        let core_err: CoreError = io_err.into();
        
        match core_err {
            CoreError::IOError(msg) => assert!(msg.contains("file missing")),
            _ => panic!("Expected IOError variant"),
        }
    }
}

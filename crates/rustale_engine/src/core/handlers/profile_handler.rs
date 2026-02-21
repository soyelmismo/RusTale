// crates/rustale_engine/src/core/handlers/profile_handler.rs

use crate::core::state::LogicState;
use crate::core::signals::FromCore;
use tokio::sync::mpsc;
use rustale_shared::profiles::ProfilesConfig;

pub async fn handle_set_current_profile(
    state: &mut LogicState,
    tx: &mpsc::Sender<FromCore>,
    uuid: uuid::Uuid,
) {
    if let Some(manager) = &mut state.profile_manager {
        if manager.set_current_profile(uuid) {
            if let Err(e) = manager.save_profiles().await {
                let _ = tx.send(FromCore::Error {
                    message: format!("Failed to set profile: {}", e),
                    fatal: false
                }).await;
            } else {
                let _ = tx.send(FromCore::ProfilesUpdated(manager.get_config())).await;
            }
        }
    }
}

pub async fn handle_create_profile(
    state: &mut LogicState,
    tx: &mpsc::Sender<FromCore>,
    name: String
) {
    if let Some(manager) = &mut state.profile_manager {
        let _ = manager.create_profile(name);
        if let Err(e) = manager.save_profiles().await {
            let _ = tx.send(FromCore::Error {
                message: format!("Save failed: {}", e),
                fatal: false
            }).await;
        } else {
            let _ = tx.send(FromCore::ProfilesUpdated(manager.get_config())).await;
        }
    }
}

pub async fn handle_delete_profile(
    state: &mut LogicState,
    tx: &mpsc::Sender<FromCore>,
    uuid: uuid::Uuid,
) {
    if let Some(manager) = &mut state.profile_manager {
        manager.delete_profile(uuid);
        if let Err(e) = manager.save_profiles().await {
            let _ = tx.send(FromCore::Error {
                message: format!("Delete failed: {}", e),
                fatal: false
            }).await;
        } else {
            let _ = tx.send(FromCore::ProfilesUpdated(manager.get_config())).await;
        }
    }
}

pub async fn handle_update_profile_name(
    state: &mut LogicState,
    tx: &mpsc::Sender<FromCore>,
    uuid: uuid::Uuid,
    name: String,
) {
    if let Some(manager) = &mut state.profile_manager {
        if manager.update_profile(uuid, name).is_some() {
            if let Err(e) = manager.save_profiles().await {
                let _ = tx.send(FromCore::Error {
                    message: format!("Update failed: {}", e),
                    fatal: false
                }).await;
            } else {
                let _ = tx.send(FromCore::ProfilesUpdated(manager.get_config())).await;
            }
        }
    }
}

pub async fn handle_save_profile(
    state: &mut LogicState,
    tx: &mpsc::Sender<FromCore>,
    config: ProfilesConfig,
) {
    if let Some(manager) = &mut state.profile_manager {
        manager.profiles = config;
        if let Err(e) = manager.save_profiles().await {
            let _ = tx.send(FromCore::Error {
                message: format!("Save failed: {}", e),
                fatal: false
            }).await;
        } else {
            let _ = tx.send(FromCore::ProfilesUpdated(manager.get_config())).await;
        }
    } else {
        // Emergency save if manager not init
        let cfg = config.clone();
        let _ = crate::system::save_profiles(&config).await;
        let _ = tx.send(FromCore::ProfilesUpdated(cfg)).await;
    }
}

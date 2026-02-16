/// Logic handlers for core business operations
/// This module contains all the heavy Task-creating logic that was previously in main.rs

use crate::config::{self, GameSettings};
use crate::game::{self, install::InstallPolicy, GamePaths, LauncherStatus};
use crate::{Message, RusTale};
use crate::services::Services;
use iced::Task;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

impl RusTale {
    /// Handles the game start logic
    /// This creates the async task for launching the game
    pub(crate) fn logic_start_game(&mut self) -> Task<Message> {
        let mut tasks = Vec::new();

        println!(
            "[Game] StartGame requested - Current status: {:?}",
            self.status
        );

        // Enhanced state validation
        if self.status == LauncherStatus::Playing {
            println!("[Game] Already playing, stopping current game...");
            self.running_game = None;
            self.status = LauncherStatus::Ready;
            self.status_text = self.localization.t("launcher.status.ready").to_string();
            self.last_status_change = std::time::Instant::now();
        } else {
            // Reset cancellation token for new launch
            self.cancellation_token.store(false, Ordering::Relaxed);
            self.error = None; // Clear any previous errors

            let player_name = self.profiles.get_current_profile_name();
            let player_uuid = self.profiles.current_profile.clone();
            let settings = self.settings.clone();
            let target_ver = self.latest_version;

            let trigger_status = self.status.clone();

            println!("[Game] Launching with profile: {}", player_name);

            self.status_text = self
                .localization
                .t("launcher.status.initializing")
                .to_string();
            self.last_status_change = std::time::Instant::now();

            if self.is_quickplay_mode {
                self.is_window_visible = false;
                tasks.push(
                    iced::window::oldest()
                        .and_then(|id| iced::window::set_mode(id, iced::window::Mode::Hidden)),
                );
            }

            // Store launch attempt with timestamp for timeout detection
            self.running_game = Some((
                settings,
                player_name,
                player_uuid.to_string(),
                target_ver,
                trigger_status,
            ));
            self.status = LauncherStatus::Busy;

            println!("[Game] Game launch initiated successfully");
        }
        Task::batch(tasks)
    }

    /// Handles the status check logic
    /// This creates the async task for checking game installation status
    pub(crate) fn logic_check_status(&mut self) -> Task<Message> {
        // Enhanced safety check: Allow status re-check if potentially stuck
        let is_potentially_stuck = match self.status {
            LauncherStatus::Playing => {
                // If "playing" but no running_game, we're in inconsistent state
                self.running_game.is_none()
            }
            LauncherStatus::Busy => {
                // If "busy" for more than 30 seconds, allow re-check
                self.running_game.is_none()
                    && self.last_status_change.elapsed().as_secs() > 30
            }
            _ => false,
        };

        if (self.status == LauncherStatus::Playing || self.running_game.is_some())
            && !is_potentially_stuck
        {
            println!(
                "[Status] Check skipped: status={:?}, running_game={:?}",
                self.status,
                self.running_game.is_some()
            );
            return Task::none();
        }

        println!("[Status] Starting status check...");
        self.last_status_change = std::time::Instant::now();
        self.status = LauncherStatus::Checking;
        self.status_text = self.localization.t("launcher.status.checking").to_string();

        let settings = self.settings.clone();
        let settings_for_closure = settings.clone();
        let paths = self.paths.clone();
        let client = self.services.api_client.clone();

        // CAMBIO: Pasamos el latest_version que ya tenemos en memoria (si existe)
        let cached_version = self.latest_version;

        Task::perform(
            async move {
                game::calculate_status(&client, &settings, &paths, cached_version).await
            },
            move |(status, latest)| {
                Message::DryRunFinished(settings_for_closure, status, latest)
            },
        )
    }

    /// Handles version check request
    pub(crate) fn logic_request_version_check(&self, channel: String) -> Task<Message> {
        let frontend = self.services.patcher.clone();
        
        Task::perform(
            async move {
                match frontend.find_latest_version(&channel, None).await {
                    Ok(latest) => {
                        let mut versions = Vec::new();
                        for i in (1..=latest).rev().take(50) {
                            versions.push(i);
                        }
                        Message::VersionsReceived(versions)
                    }
                    Err(e) => {
                        eprintln!("Failed to fetch versions: {}", e);
                        Message::VersionsReceived(Vec::new())
                    }
                }
            },
            |msg| msg,
        )
    }

    /// Handles version repair request
    pub(crate) fn logic_request_repair_version(&mut self, version: u32) -> Task<Message> {
        let base_dir = config::get_app_dir();
        let channel = self.settings.channel.clone();
        let client = self.services.download_client.clone();
        let frontend = self.services.patcher.clone();
        let cancel_token = self.cancellation_token.clone();

        // Change status to Downloading immediately so cancel button appears
        self.status = LauncherStatus::Downloading;
        self.status_text = "Starting repair...".to_string();
        self.download_progress = 0.1;

        Task::perform(
            async move {
                let _version_str = if version == 0 {
                    "latest".to_string()
                } else {
                    version.to_string()
                };

                // Re-download and verify the version using the new progress system
                // For repair operations, we'll collect progress updates and return them
                let result = frontend
                    .ensure_installed_with_weighted_progress(
                        &client,
                        &base_dir,
                        &channel,
                        Some(version as i32),
                        InstallPolicy::NetworkUpdate,
                        |payload| {
                            // For repair operations, we'll log progress
                            // The UI updates will be handled by the main download flow
                            println!("[Repair] {:.1}% - {}", 
                                payload.global_progress * 100.0, 
                                payload.message_key
                            );
                        },
                        Some(cancel_token),
                    )
                    .await;

                match result {
                    Ok(_) => Message::RepairFinished(Ok(())),
                    Err(e) => Message::RepairFinished(Err(e.to_string())),
                }
            },
            |msg| msg,
        )
    }

    /// Handles version deletion request
    pub(crate) fn logic_request_delete_version(&self, version: u32) -> Task<Message> {
        let base_dir = config::get_app_dir();
        let channel = self.settings.channel.clone();

        Task::perform(
            async move {
                let version_str = if version == 0 {
                    "latest".to_string()
                } else {
                    version.to_string()
                };

                let paths = GamePaths::new(base_dir);
                let version_dir = paths.version_dir(&channel, &version_str);

                if version_dir.exists() {
                    if let Err(e) = tokio::fs::remove_dir_all(&version_dir).await {
                        eprintln!("Failed to delete version {}: {}", version, e);
                    } else {
                        println!("Deleted version {} successfully", version);
                    }
                }

                // Refresh the installed versions list
                Message::Settings(crate::settings::SettingsMessage::VersionSelected(0))
            },
            |msg| msg,
        )
    }

    /// Handles Java info loading
    pub(crate) fn logic_load_java_info() -> Task<Message> {
        let base_dir = config::get_app_dir();
        Task::perform(
            async move {
                match crate::java_detection::ensure_java_available(&base_dir).await {
                    Ok(java_info) => Message::Settings(
                        crate::settings::SettingsMessage::JavaVersionUpdated(java_info.version),
                    ),
                    Err(e) => {
                        eprintln!("Java detection/download failed: {}", e);
                        Message::Settings(crate::settings::SettingsMessage::JavaInfoLoaded)
                    }
                }
            },
            |msg| msg,
        )
    }

    /// Handles data migration request
    pub(crate) fn logic_start_migration(
        &mut self,
        from: std::path::PathBuf,
        to: std::path::PathBuf,
    ) -> Task<Message> {
        self.status = LauncherStatus::Busy;
        self.status_text = "Moving data...".to_string();

        Task::perform(
            async move {
                match crate::util::move_dir_with_progress(from, to.clone(), |_| {}).await {
                    Ok(()) => Message::DataMoveFinished(Ok(to)),
                    Err(e) => Message::DataMoveFinished(Err(e.to_string())),
                }
            },
            |msg| msg,
        )
    }
}

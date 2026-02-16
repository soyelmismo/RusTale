/// Logic handlers for core business operations
/// This module contains all the heavy Task-creating logic that was previously in main.rs

use crate::config::{self, GameSettings};
use crate::game::{self, install::InstallPolicy, GamePaths, LauncherStatus};
use crate::main::{Message, RusTale};
use crate::services::Services;
use iced::Task;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

impl RusTale {
    /// Handles the game start logic
    /// This creates the async task for launching the game
    pub(crate) fn logic_start_game(\u0026mut self) -\u003e Task<Message> {
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
    pub(crate) fn logic_check_status(\u0026mut self) -\u003e Task<Message> {
        // Enhanced safety check: Allow status re-check if potentially stuck
        let is_potentially_stuck = match self.status {
            LauncherStatus::Playing =\u003e {
                // If "playing" but no running_game, we're in inconsistent state
                self.running_game.is_none()
            }
            LauncherStatus::Busy =\u003e {
                // If "busy" for more than 30 seconds, allow re-check
                self.running_game.is_none()
                    \u0026\u0026 self.last_status_change.elapsed().as_secs() \u003e 30
            }
            _ =\u003e false,
        };

        if (self.status == LauncherStatus::Playing || self.running_game.is_some())
            \u0026\u0026 !is_potentially_stuck
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
                game::calculate_status(\u0026client, \u0026settings, \u0026paths, cached_version).await
            },
            move |(status, latest)| {
                Message::DryRunFinished(settings_for_closure, status, latest)
            },
        )
    }

    /// Handles version check request
    pub(crate) fn logic_request_version_check(\u0026self, channel: String) -\u003e Task<Message> {
        let frontend = self.services.patcher.clone();
        
        Task::perform(
            async move {
                match frontend.find_latest_version(\u0026channel, None).await {
                    Ok(latest) =\u003e {
                        let mut versions = Vec::new();
                        for i in (1..=latest).rev().take(50) {
                            versions.push(i);
                        }
                        Message::VersionsReceived(versions)
                    }
                    Err(e) =\u003e {
                        eprintln!("Failed to fetch versions: {}", e);
                        Message::VersionsReceived(Vec::new())
                    }
                }
            },
            |msg| msg,
        )
    }

    /// Handles version repair request
    pub(crate) fn logic_request_repair_version(\u0026self, version: u32) -\u003e Task<Message> {
        let base_dir = config::get_app_dir();
        let channel = self.settings.channel.clone();
        let client = self.services.download_client.clone();
        let frontend = self.services.patcher.clone();
        let cancel_token = Arc::new(AtomicBool::new(false));

        Task::perform(
            async move {
                let version_str = if version == 0 {
                    "latest".to_string()
                } else {
                    version.to_string()
                };

                // Re-download and verify the version
                let result = frontend
                    .ensure_installed(
                        \u0026client,
                        \u0026base_dir,
                        \u0026channel,
                        Some(version as i32),
                        InstallPolicy::NetworkUpdate,
                        |_step, _prog, _speed, _total, _down, _eta, _current_step| {
                            // Progress callback - could be enhanced to send progress messages
                        },
                        Some(cancel_token),
                    )
                    .await;

                match result {
                    Ok(_) =\u003e Message::RepairFinished(Ok(())),
                    Err(e) =\u003e Message::RepairFinished(Err(e.to_string())),
                }
            },
            |msg| msg,
        )
    }

    /// Handles version deletion request
    pub(crate) fn logic_request_delete_version(\u0026self, version: u32) -\u003e Task<Message> {
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
                let version_dir = paths.version_dir(\u0026channel, \u0026version_str);

                if version_dir.exists() {
                    if let Err(e) = tokio::fs::remove_dir_all(\u0026version_dir).await {
                        eprintln!("Failed to delete version {}: {}", version, e);
                    } else {
                        println!("Deleted version {} successfully", version);
                    }
                }

                // Refresh the installed versions list
                Message::Settings(crate::settings::SettingsMessage::RefreshInstalledVersions)
            },
            |msg| msg,
        )
    }

    /// Handles Java info loading
    pub(crate) fn logic_load_java_info() -\u003e Task<Message> {
        let base_dir = config::get_app_dir();
        Task::perform(
            async move {
                match crate::java_detection::ensure_java_available(\u0026base_dir).await {
                    Ok(java_info) =\u003e Message::Settings(
                        crate::settings::SettingsMessage::JavaVersionUpdated(java_info.version),
                    ),
                    Err(e) =\u003e {
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
        \u0026mut self,
        from: std::path::PathBuf,
        to: std::path::PathBuf,
    ) -\u003e Task<Message> {
        self.status = LauncherStatus::Busy;
        self.status_text = "Moving data...".to_string();

        Task::perform(
            async move {
                match crate::util::move_data_with_progress(from, to).await {
                    Ok(new_path) =\u003e Message::DataMoveFinished(Ok(new_path)),
                    Err(e) =\u003e Message::DataMoveFinished(Err(e.to_string())),
                }
            },
            |msg| msg,
        )
    }
}

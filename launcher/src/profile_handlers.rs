/// Profile management handlers
/// Extracts profile-related logic from main.rs

use crate::config::{self, Profile};
use crate::{Message, RusTale};
use iced::{clipboard, Task};

impl RusTale {
    /// Handles all profile-related messages
    pub(crate) fn handle_profile_message_ext(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::ProfileSelected(profile) => {
                self.profiles.current_profile = profile.id;
                self.profile_dropdown_open = false;

                // Reconcile local server when profile changes
                self.reconcile_local_server();

                // Save profiles asynchronously
                let profiles = self.profiles.clone();
                Task::perform(
                    async move {
                        config::save_profiles(&profiles).await;
                    },
                    |_| Message::None,
                )
            }

            Message::AddProfile => {
                self.editing_profile = Some((None, String::new()));
                self.profile_dropdown_open = false;
                Task::none()
            }

            Message::EditProfile(id) => {
                if let Some(profile) = self.profiles.profiles.iter().find(|p| p.id == id) {
                    self.editing_profile = Some((Some(id), profile.name.clone()));
                }
                self.profile_dropdown_open = false;
                Task::none()
            }

            Message::DeleteProfile(id) => {
                // Don't allow deleting the last profile
                if self.profiles.profiles.len() > 1 {
                    self.profiles.profiles.retain(|p| p.id != id);

                    // If we deleted the current profile, switch to the first one
                    if self.profiles.current_profile == id {
                        if let Some(first) = self.profiles.profiles.first() {
                            self.profiles.current_profile = first.id;
                        }
                    }

                    // Save profiles asynchronously
                    let profiles = self.profiles.clone();
                    return Task::perform(
                        async move {
                            config::save_profiles(&profiles).await;
                        },
                        |_| Message::None,
                    );
                }
                Task::none()
            }

            Message::ProfileNameChanged(name) => {
                if let Some((_, ref mut current_name)) = self.editing_profile {
                    *current_name = name;
                }
                Task::none()
            }

            Message::SaveProfileName => {
                if let Some((id, name)) = self.editing_profile.take() {
                    if !name.trim().is_empty() {
                        match id {
                            Some(existing_id) => {
                                // Edit existing profile
                                if let Some(profile) =
                                    self.profiles.profiles.iter_mut().find(|p| p.id == existing_id)
                                {
                                    profile.name = name;
                                }
                            }
                            None => {
                                // Create new profile
                                let new_profile = Profile {
                                    id: uuid::Uuid::new_v4(),
                                    name,
                                };
                                self.profiles.profiles.push(new_profile.clone());
                                self.profiles.current_profile = new_profile.id;
                            }
                        }

                        // Save profiles asynchronously
                        let profiles = self.profiles.clone();
                        return Task::perform(
                            async move {
                                config::save_profiles(&profiles).await;
                            },
                            |_| Message::None,
                        );
                    }
                }
                Task::none()
            }

            Message::CancelProfileEdit => {
                self.editing_profile = None;
                Task::none()
            }

            Message::EditProfileUUID(id) => {
                if let Some(profile) = self.profiles.profiles.iter().find(|p| p.id == id) {
                    self.editing_uuid = Some((id, profile.id.to_string()));
                }
                self.profile_dropdown_open = false;
                Task::none()
            }

            Message::ProfileUUIDChanged(uuid_str) => {
                if let Some((_, ref mut current_uuid)) = self.editing_uuid {
                    *current_uuid = uuid_str;
                }
                Task::none()
            }

            Message::SaveProfileUUID => {
                if let Some((id, uuid_str)) = self.editing_uuid.take() {
                    if let Ok(new_uuid) = uuid::Uuid::parse_str(&uuid_str) {
                        if let Some(profile) = self.profiles.profiles.iter_mut().find(|p| p.id == id)
                        {
                            profile.id = new_uuid;

                            // If this was the current profile, update the current_profile ID
                            if self.profiles.current_profile == id {
                                self.profiles.current_profile = new_uuid;
                            }

                            // Save profiles asynchronously
                            let profiles = self.profiles.clone();
                            return Task::perform(
                                async move {
                                    config::save_profiles(&profiles).await;
                                },
                                |_| Message::None,
                            );
                        }
                    }
                }
                Task::none()
            }

            Message::CancelProfileUUIDEdit => {
                self.editing_uuid = None;
                Task::none()
            }

            Message::CopyUUID(uuid) => clipboard::write(uuid),

            Message::GenerateRandomUUID => {
                if let Some((_, ref mut uuid_str)) = self.editing_uuid {
                    *uuid_str = uuid::Uuid::new_v4().to_string();
                }
                Task::none()
            }

            Message::ToggleProfileDropdown => {
                self.profile_dropdown_open = !self.profile_dropdown_open;
                Task::none()
            }

            _ => Task::none(),
        }
    }
}

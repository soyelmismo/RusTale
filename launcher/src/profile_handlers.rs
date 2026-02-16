/// Profile management handlers
/// Extracts profile-related logic from main.rs

use crate::config::{self, Profile};
use crate::main::{Message, RusTale};
use iced::{clipboard, Task};

impl RusTale {
    /// Handles all profile-related messages
    pub(crate) fn handle_profile_message(\u0026mut self, message: Message) -\u003e Task<Message> {
        match message {
            Message::ProfileSelected(profile) =\u003e {
                self.profiles.current_profile = profile.id;
                self.profile_dropdown_open = false;

                // Reconcile local server when profile changes
                self.reconcile_local_server();

                // Save profiles asynchronously
                let profiles = self.profiles.clone();
                Task::perform(
                    async move {
                        config::save_profiles(\u0026profiles).await;
                    },
                    |_| Message::None,
                )
            }

            Message::AddProfile =\u003e {
                self.editing_profile = Some((None, String::new()));
                self.profile_dropdown_open = false;
                Task::none()
            }

            Message::EditProfile(id) =\u003e {
                if let Some(profile) = self.profiles.profiles.iter().find(|p| p.id == id) {
                    self.editing_profile = Some((Some(id), profile.name.clone()));
                }
                self.profile_dropdown_open = false;
                Task::none()
            }

            Message::DeleteProfile(id) =\u003e {
                // Don't allow deleting the last profile
                if self.profiles.profiles.len() \u003e 1 {
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
                            config::save_profiles(\u0026profiles).await;
                        },
                        |_| Message::None,
                    );
                }
                Task::none()
            }

            Message::ProfileNameChanged(name) =\u003e {
                if let Some((_, ref mut current_name)) = self.editing_profile {
                    *current_name = name;
                }
                Task::none()
            }

            Message::SaveProfileName =\u003e {
                if let Some((id, name)) = self.editing_profile.take() {
                    if !name.trim().is_empty() {
                        match id {
                            Some(existing_id) =\u003e {
                                // Edit existing profile
                                if let Some(profile) =
                                    self.profiles.profiles.iter_mut().find(|p| p.id == existing_id)
                                {
                                    profile.name = name;
                                }
                            }
                            None =\u003e {
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
                                config::save_profiles(\u0026profiles).await;
                            },
                            |_| Message::None,
                        );
                    }
                }
                Task::none()
            }

            Message::CancelProfileEdit =\u003e {
                self.editing_profile = None;
                Task::none()
            }

            Message::EditProfileUUID(id) =\u003e {
                if let Some(profile) = self.profiles.profiles.iter().find(|p| p.id == id) {
                    self.editing_uuid = Some((id, profile.id.to_string()));
                }
                self.profile_dropdown_open = false;
                Task::none()
            }

            Message::ProfileUUIDChanged(uuid_str) =\u003e {
                if let Some((_, ref mut current_uuid)) = self.editing_uuid {
                    *current_uuid = uuid_str;
                }
                Task::none()
            }

            Message::SaveProfileUUID =\u003e {
                if let Some((id, uuid_str)) = self.editing_uuid.take() {
                    if let Ok(new_uuid) = uuid::Uuid::parse_str(\u0026uuid_str) {
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
                                    config::save_profiles(\u0026profiles).await;
                                },
                                |_| Message::None,
                            );
                        }
                    }
                }
                Task::none()
            }

            Message::CancelProfileUUIDEdit =\u003e {
                self.editing_uuid = None;
                Task::none()
            }

            Message::CopyUUID(uuid) =\u003e clipboard::write(uuid),

            Message::GenerateRandomUUID =\u003e {
                if let Some((_, ref mut uuid_str)) = self.editing_uuid {
                    *uuid_str = uuid::Uuid::new_v4().to_string();
                }
                Task::none()
            }

            Message::ToggleProfileDropdown =\u003e {
                self.profile_dropdown_open = !self.profile_dropdown_open;
                Task::none()
            }

            _ =\u003e Task::none(),
        }
    }
}

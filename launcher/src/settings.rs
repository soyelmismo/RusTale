use crate::Message;
use crate::config::GameSettings;
use crate::{theme, util};
use iced::widget::{
    Space, button, checkbox, column, container, pick_list, row, scrollable, slider, svg, text,
    text_input,
};
use iced::{Alignment, Color, Element, Length, Size};

#[derive(Debug, Clone, PartialEq)]
pub enum Tab {
    Launcher,
    Game,
    Video,
    Java,
}

#[derive(Debug, Clone)]
pub enum SettingsMessage {
    TabSelected(Tab),
    // Game
    ChannelChanged(String),
    EnableNewsToggled(bool),
    EnableOnlineFixToggled(bool),
    // Video
    WidthChanged(String),
    HeightChanged(String),
    FullscreenToggled(bool),
    // Java
    MinMemoryChanged(f32),
    MaxMemoryChanged(f32),
    JavaArgsChanged(String),

    CloseModal,
    SaveSettings,

    VersionSelected(u32),
    DeleteVersion(u32),
    OpenVersionFolder(u32),

    BrowseInstallPath,
    PathSelected(std::path::PathBuf),
    LanguageSelected(crate::lang::Language),
    OnlineFixModeChanged(crate::config::OnlineFixMode),
    ToggleMinimizeTray(bool),
    ToggleMinimizePlay(bool),
    QuickplayToggled(bool),
    AutoUpdateToggled(bool),
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionOption {
    Latest,
    Specific(u32),
}

impl std::fmt::Display for VersionOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // En este impl no tenemos acceso a la Localization fácilmente sin cambiar muchas firmas.
        // Pero como se usa principalmente en PickList, el PickList puede usar una función de mapeo manual.
        // Por ahora lo dejamos igual o usamos placeholders que luego se traducen en el view.
        match self {
            Self::Latest => write!(f, "Latest"),
            Self::Specific(v) => write!(f, "{}", v),
        }
    }
}

impl From<u32> for VersionOption {
    fn from(v: u32) -> Self {
        if v == 0 {
            Self::Latest
        } else {
            Self::Specific(v)
        }
    }
}

impl From<VersionOption> for u32 {
    fn from(opt: VersionOption) -> Self {
        match opt {
            VersionOption::Latest => 0,
            VersionOption::Specific(v) => v,
        }
    }
}

// Estado local del modal
pub struct SettingsState {
    pub current_tab: Tab,
    pub temp_settings: GameSettings,
    pub is_open: bool,
    pub available_versions: Vec<i32>,
    pub installed_versions: Vec<(i32, bool)>, // (version, is_latest_folder)
    pub is_loading_versions: bool,
    pub new_install_path: String,
}

impl SettingsState {
    pub fn new(current_settings: GameSettings) -> Self {
        Self {
            current_tab: Tab::Game,
            temp_settings: current_settings,
            available_versions: Vec::new(),
            installed_versions: Vec::new(),
            is_open: false,
            is_loading_versions: false,
            new_install_path: String::new(),
        }
    }

    pub fn open(&mut self, settings: GameSettings) {
        self.temp_settings = settings;
        self.new_install_path = crate::config::get_app_dir().to_string_lossy().to_string();
        self.current_tab = Tab::Launcher; // Opcional: volver siempre a la primera pestana
        self.is_open = true;
        // installed_versions should be updated via message shortly after opening
    }

    fn view_launcher_tab<'a>(
        &'a self,
        localization: &'a crate::lang::Localization,
        _is_compact: bool, // Recibimos el parámetro
    ) -> Element<'a, SettingsMessage> {
        // --- Language Picker ---
        let selected_language = localization
            .available_languages
            .iter()
            .find(|l| l.id == self.temp_settings.language)
            .cloned();

        let language_pick = pick_list(
            &localization.available_languages[..],
            selected_language,
            SettingsMessage::LanguageSelected,
        )
        .text_size(14) // Tamaño de texto explícito
        .placeholder(localization.t("settings.language.placeholder"))
        .padding(10)
        .width(150)
        .style(theme::pick_list_style)
        .menu_style(theme::menu_style);

        let news_checkbox = row![
            checkbox(self.temp_settings.enable_news)
                .on_toggle(SettingsMessage::EnableNewsToggled)
                .style(theme::checkbox_style),
            text(localization.t("settings.enable_news")).size(14),
        ]
        .spacing(10)
        .align_y(Alignment::Center);

        let minimize_tray_chk = row![
            checkbox(self.temp_settings.minimize_to_tray)
                .on_toggle(SettingsMessage::ToggleMinimizeTray)
                .style(theme::checkbox_style),
            text(localization.t("settings.minimize_to_tray")).size(14)
        ]
        .spacing(10)
        .align_y(Alignment::Center);

        let minimize_play_chk = row![
            checkbox(self.temp_settings.minimize_on_play)
                .on_toggle(SettingsMessage::ToggleMinimizePlay)
                .style(theme::checkbox_style),
            text(localization.t("settings.minimize_on_play")).size(14)
        ]
        .spacing(10)
        .align_y(Alignment::Center);

        let auto_update_chk = row![
            checkbox(self.temp_settings.enable_auto_update)
                .on_toggle(SettingsMessage::AutoUpdateToggled)
                .style(theme::checkbox_style),
            text(localization.t("settings.auto_update")).size(14),
        ]
        .spacing(10)
        .align_y(Alignment::Center);

        let quickplay_chk = row![
            checkbox(self.temp_settings.quickplay)
                .on_toggle(SettingsMessage::QuickplayToggled)
                .style(theme::checkbox_style),
            column![
                text(localization.t("settings.quickplay")).size(14),
                text(localization.t("settings.quickplay_desc"))
                    .size(10)
                    .color(Color::from_rgb(0.5, 0.5, 0.5)),
            ]
        ]
        .spacing(10)
        .align_y(Alignment::Center);

        column![
            section_title(localization.t("settings.tabs.launcher")),
            column![
                text(localization.t("settings.language"))
                    .size(12)
                    .color(Color::from_rgb(0.7, 0.7, 0.7)),
                language_pick
            ]
            .spacing(5),
            Space::new().height(10),
            news_checkbox,
            auto_update_chk,
            minimize_tray_chk,
            minimize_play_chk,
            quickplay_chk,
        ]
        .spacing(15)
        .width(Length::Fill)
        .into()
    }

    fn view_game_tab<'a>(
        &'a self,
        localization: &'a crate::lang::Localization,
        is_compact: bool,
    ) -> Element<'a, SettingsMessage> {
        // --- 1. Installation Path Selector ---
        let path_selector = column![
            section_title(localization.t("settings.install_path")),
            row![
                text_input(
                    localization.t("settings.game.path_field"),
                    &self.new_install_path
                )
                .on_input(|_| SettingsMessage::BrowseInstallPath)
                .size(14) // Text Size ajustado
                .style(theme::text_input_style)
                .width(Length::Fill),
                button(text(localization.t("settings.browse")).size(14))
                    .on_press(SettingsMessage::BrowseInstallPath)
                    .style(theme::secondary_button_style)
            ]
            .spacing(10)
        ]
        .spacing(5);

        // --- 2. Channel & Version Pickers ---
        // Create "Channel Section" as a typed Element to resolve inference immediately
        let channel_section: Element<'a, SettingsMessage> = column![
            text(localization.t("settings.update_channel"))
                .size(12)
                .color(Color::from_rgb(0.7, 0.7, 0.7)),
            pick_list(
                vec!["pre-release", "release"],
                Some(self.temp_settings.channel.as_str()),
                |c| SettingsMessage::ChannelChanged(c.to_string()),
            )
            .text_size(14)
            .placeholder(localization.t("settings.select_channel"))
            .padding(10)
            .width(if is_compact {
                Length::Fill
            } else {
                Length::Fixed(150.0)
            })
            .style(theme::pick_list_style)
            .menu_style(theme::menu_style)
        ]
        .spacing(5)
        .width(if is_compact {
            Length::Fill
        } else {
            Length::Shrink
        })
        .into();

        // Prepare version options: 0 (Latest) + available versions
        #[derive(Debug, Clone, PartialEq, Eq)]
        struct LocalizedVersion {
            option: VersionOption,
            label: String,
        }

        impl std::fmt::Display for LocalizedVersion {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.label)
            }
        }

        let mut version_options = Vec::new();

        if self.is_loading_versions {
            version_options.push(LocalizedVersion {
                option: VersionOption::Latest,
                label: "Searching updates...".to_string(),
            });
        } else {
            version_options.push(LocalizedVersion {
                option: VersionOption::Latest,
                label: localization.t("settings.version.latest").to_string(),
            });

            version_options.extend(self.available_versions.iter().map(|&v| {
                let opt = VersionOption::Specific(v as u32);
                LocalizedVersion {
                    option: opt,
                    label: localization.ta("settings.version.specific", &[&v.to_string()]),
                }
            }));
        }

        let selected_version = VersionOption::from(self.temp_settings.game_version);

        let selected_localized = if self.is_loading_versions {
            version_options.first().cloned()
        } else {
            version_options
                .iter()
                .find(|v| v.option == selected_version)
                .cloned()
        };

        // Create "Version Section" as a typed Element
        let version_section: Element<'a, SettingsMessage> = column![
            text(localization.t("settings.target_version"))
                .size(12)
                .color(Color::from_rgb(0.7, 0.7, 0.7)),
            pick_list(version_options, selected_localized, |v| {
                if v.label.starts_with("Searching") {
                    return SettingsMessage::None;
                }
                SettingsMessage::VersionSelected(u32::from(v.option))
            })
            .text_size(14)
            .placeholder(if self.is_loading_versions {
                "Searching..."
            } else {
                localization.t("settings.select_version")
            })
            .padding(10)
            .width(if is_compact {
                Length::Fill
            } else {
                Length::Fixed(150.0)
            })
            .style(theme::pick_list_style)
            .menu_style(theme::menu_style)
        ]
        .spacing(5)
        .width(if is_compact {
            Length::Fill
        } else {
            Length::Shrink
        })
        .into();

        let modes = vec![
            crate::config::OnlineFixMode::Local,
            crate::config::OnlineFixMode::Sanasol,
        ];

        let fix_mode_pick = pick_list(
            modes,
            Some(self.temp_settings.online_fix_mode.clone()),
            SettingsMessage::OnlineFixModeChanged,
        )
        .text_size(14)
        .padding(5)
        .width(150)
        .style(theme::pick_list_style)
        .menu_style(theme::menu_style);

        let online_fix_checkbox = row![
            checkbox(self.temp_settings.enable_online_fix)
                .on_toggle(SettingsMessage::EnableOnlineFixToggled)
                .style(theme::checkbox_style),
            column![
                text(localization.t("settings.enable_online_fix")).size(14),
                text(localization.t("settings.online_fix_desc"))
                    .size(10)
                    .color(Color::from_rgb(0.5, 0.5, 0.5)),
            ]
        ]
        .spacing(10)
        .align_y(Alignment::Center);

        let online_fix_section = column![
            online_fix_checkbox,
            if self.temp_settings.enable_online_fix {
                container(
                    row![
                        text(localization.t("settings.patch_mode"))
                            .size(12)
                            .color(Color::from_rgb(0.7, 0.7, 0.7)),
                        fix_mode_pick
                    ]
                    .spacing(10)
                    .align_y(Alignment::Center),
                )
                .padding(iced::Padding {
                    top: 0.0,
                    right: 0.0,
                    bottom: 0.0,
                    left: 30.0,
                })
            } else {
                container(Space::new())
            },
        ]
        .spacing(10);

        // Layout adaptable para Channel y Version
        let version_controls: Element<'a, SettingsMessage> = if is_compact {
            column![channel_section, version_section].spacing(10).into()
        } else {
            row![channel_section, version_section].spacing(20).into()
        };

        let game_config = column![
            section_title(localization.t("settings.game_config")),
            online_fix_section,
            Space::new().height(5),
            version_controls,
            Space::new().height(5),
        ]
        .spacing(10);

        // --- 3. Storage Management (Installed Versions) ---
        let installed_content: Element<'_, SettingsMessage> = if self.installed_versions.is_empty()
        {
            container(
                text(localization.t("settings.storage.no_versions"))
                    .size(12)
                    .color(Color::from_rgb(0.5, 0.5, 0.5)),
            )
            .width(Length::Fill)
            .padding(10)
            .into()
        } else {
            let mut list = column!().spacing(5);
            for &(version, is_latest) in &self.installed_versions {
                let label = if is_latest {
                    localization.ta("settings.storage.latest_format", &[&version.to_string()])
                } else {
                    localization.ta("settings.storage.version_format", &[&version.to_string()])
                };

                let version_val = if is_latest { 0 } else { version as u32 };

                list = list.push(
                    container(
                        row![
                            text(label).size(12).width(Length::Fill),
                            button(
                                container(
                                    svg(util::icons::icon(util::icons::FOLDER))
                                        .style(theme::svg_accent),
                                )
                                .center_x(Length::Fill)
                                .center_y(Length::Fill)
                            )
                            .on_press(SettingsMessage::OpenVersionFolder(version_val))
                            .style(theme::secondary_button_style),
                            Space::new().width(5),
                            button(
                                container(
                                    svg(util::icons::icon(util::icons::TRASH))
                                        .style(theme::svg_accent),
                                )
                                .center_x(Length::Fill)
                                .center_y(Length::Fill)
                            )
                            .on_press(SettingsMessage::DeleteVersion(version_val))
                            .style(theme::secondary_button_style),
                        ]
                        .align_y(Alignment::Center),
                    )
                    .padding(5)
                    .style(theme::sidebar_style)
                    .width(Length::Fill)
                    .height(40),
                );
            }
            scrollable(list)
                .height(120)
                .style(theme::scrollable_style)
                .into()
        };

        let installed_manager = column![
            section_title(localization.t("settings.storage.title")),
            text(localization.t("settings.storage.desc"))
                .size(12)
                .color(Color::from_rgb(0.5, 0.5, 0.5)),
            Space::new().height(5),
            installed_content
        ]
        .spacing(5);

        column![
            path_selector,
            Space::new().height(15),
            game_config,
            Space::new().height(15),
            installed_manager
        ]
        .spacing(10)
        .width(Length::Fill)
        .into()
    }

    pub fn update(&mut self, message: SettingsMessage) -> Option<Message> {
        match message {
            SettingsMessage::TabSelected(tab) => {
                self.current_tab = tab;
                None
            }
            SettingsMessage::ChannelChanged(val) => {
                self.temp_settings.channel = val.clone();
                self.is_loading_versions = true;
                self.available_versions.clear();
                Some(Message::RequestVersionCheck(val))
            }
            SettingsMessage::EnableNewsToggled(val) => {
                self.temp_settings.enable_news = val;
                None
            }
            SettingsMessage::EnableOnlineFixToggled(val) => {
                self.temp_settings.enable_online_fix = val;
                None
            }
            SettingsMessage::OnlineFixModeChanged(mode) => {
                self.temp_settings.online_fix_mode = mode;
                None
            }
            SettingsMessage::VersionSelected(version) => {
                self.temp_settings.game_version = version;
                Some(Message::CheckStatus)
            }
            SettingsMessage::CloseModal => {
                self.is_open = false;
                Some(Message::CloseSettings)
            }
            SettingsMessage::SaveSettings => {
                self.is_open = false;
                // Save bootstrap path if changed
                let current_dir = crate::config::get_app_dir();
                if self.new_install_path != current_dir.to_string_lossy() {
                    let path = std::path::PathBuf::from(&self.new_install_path);
                    let _ = crate::config::save_bootstrap_path(&path);
                }

                Some(Message::SaveSettings(self.temp_settings.clone()))
            }
            SettingsMessage::BrowseInstallPath => {
                Some(Message::Settings(SettingsMessage::BrowseInstallPath))
            }
            SettingsMessage::PathSelected(path) => {
                self.new_install_path = path.to_string_lossy().to_string();
                None
            }
            SettingsMessage::WidthChanged(val) => {
                if let Ok(num) = val.parse() {
                    self.temp_settings.width = num;
                }
                None
            }
            SettingsMessage::HeightChanged(val) => {
                if let Ok(num) = val.parse() {
                    self.temp_settings.height = num;
                }
                None
            }
            SettingsMessage::FullscreenToggled(val) => {
                self.temp_settings.fullscreen = val;
                None
            }
            SettingsMessage::MinMemoryChanged(val) => {
                self.temp_settings.min_memory = val as u32;
                if self.temp_settings.min_memory > self.temp_settings.max_memory {
                    self.temp_settings.max_memory = self.temp_settings.min_memory;
                }
                None
            }
            SettingsMessage::MaxMemoryChanged(val) => {
                self.temp_settings.max_memory = val as u32;
                if self.temp_settings.max_memory < self.temp_settings.min_memory {
                    self.temp_settings.min_memory = self.temp_settings.max_memory;
                }
                None
            }
            SettingsMessage::JavaArgsChanged(val) => {
                self.temp_settings.java_args = val;
                None
            }
            SettingsMessage::DeleteVersion(v) => Some(Message::RequestDeleteVersion(v)),
            SettingsMessage::OpenVersionFolder(v) => Some(Message::OpenVersionFolder(v)),
            SettingsMessage::LanguageSelected(lang) => {
                Some(Message::LanguageChangedInSettings(lang.id))
            }
            SettingsMessage::ToggleMinimizeTray(val) => {
                self.temp_settings.minimize_to_tray = val;
                None
            }
            SettingsMessage::ToggleMinimizePlay(val) => {
                self.temp_settings.minimize_on_play = val;
                None
            }
            SettingsMessage::QuickplayToggled(val) => {
                self.temp_settings.quickplay = val;
                None
            }
            SettingsMessage::AutoUpdateToggled(val) => {
                self.temp_settings.enable_auto_update = val;
                None
            }
            SettingsMessage::None => None,
        }
    }

    pub fn view<'a>(
        &'a self,
        localization: &'a crate::lang::Localization,
        window_size: Size,
    ) -> Element<'a, SettingsMessage> {
        // Detectar modo compacto
        let is_compact = window_size.width < 650.0;
        let tab_width = if is_compact { 60.0 } else { 170.0 };

        let tabs = column![
            tab_button(
                &localization.t("settings.tabs.launcher"),
                util::icons::SETTINGS,
                Tab::Launcher,
                &self.current_tab,
                is_compact
            ),
            tab_button(
                &localization.t("settings.tabs.game"),
                util::icons::GAMEPAD,
                Tab::Game,
                &self.current_tab,
                is_compact
            ),
            tab_button(
                &localization.t("settings.tabs.video"),
                util::icons::MONITOR,
                Tab::Video,
                &self.current_tab,
                is_compact
            ),
            tab_button(
                &localization.t("settings.tabs.java"),
                util::icons::COFFEE,
                Tab::Java,
                &self.current_tab,
                is_compact
            )
        ]
        .spacing(5)
        .width(tab_width);

        let content = match self.current_tab {
            Tab::Launcher => self.view_launcher_tab(localization, is_compact),
            Tab::Game => self.view_game_tab(localization, is_compact),
            Tab::Video => column![
                section_title(localization.t("settings.display")),
                row![
                    input_group(
                        localization.t("settings.width"),
                        &self.temp_settings.width.to_string(),
                        SettingsMessage::WidthChanged
                    ),
                    input_group(
                        localization.t("settings.height"),
                        &self.temp_settings.height.to_string(),
                        SettingsMessage::HeightChanged
                    ),
                ]
                .spacing(20),
                Space::new().height(20),
                row![
                    checkbox(self.temp_settings.fullscreen)
                        .on_toggle(SettingsMessage::FullscreenToggled)
                        .style(theme::checkbox_style),
                    text(localization.t("settings.fullscreen")).size(14)
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            ]
            .spacing(15)
            .into(),
            Tab::Java => column![
                section_title(localization.t("settings.java_memory")),
                text(format!(
                    "{}: {} GB",
                    localization.t("settings.min"),
                    self.temp_settings.min_memory
                ))
                .size(12),
                slider(
                    1.0..=16.0,
                    self.temp_settings.min_memory as f32,
                    SettingsMessage::MinMemoryChanged
                )
                .step(1.0)
                .style(theme::slider_style),
                Space::new().height(10),
                text(format!(
                    "{}: {} GB",
                    localization.t("settings.max"),
                    self.temp_settings.max_memory
                ))
                .size(12),
                slider(
                    1.0..=32.0,
                    self.temp_settings.max_memory as f32,
                    SettingsMessage::MaxMemoryChanged
                )
                .step(1.0)
                .style(theme::slider_style),
                Space::new().height(20),
                section_title(localization.t("settings.jvm_args")),
                text_input(
                    localization.t("settings.jvm_args_placeholder"),
                    &self.temp_settings.java_args
                )
                .on_input(SettingsMessage::JavaArgsChanged)
                .padding(10)
                .style(theme::text_input_style),
            ]
            .spacing(15)
            .into(),
        };

        let footer = row![
            button(text(localization.t("settings.cancel")).size(14))
                .on_press(SettingsMessage::CloseModal)
                .style(theme::secondary_button_style)
                .padding(10), // Reduced visible scaling by keeping padding reasonable but text small
            Space::new().width(10),
            button(text(localization.t("settings.save")).size(14))
                .on_press(SettingsMessage::SaveSettings)
                .style(theme::primary_button_style)
                .padding(10)
        ]
        .align_y(Alignment::Center)
        .width(Length::Fill)
        .padding(10);

        let modal_width = if is_compact {
            Length::Fill
        } else {
            Length::Fixed(700.0)
        };
        let modal_height = if window_size.height < 550.0 {
            Length::Fill
        } else {
            Length::Fixed(500.0)
        };
        let padding_outer = if is_compact { 10 } else { 0 };

        container(column![
            row![
                text(localization.t("settings.title"))
                    .size(18)
                    .font(iced::font::Font::MONOSPACE),
                Space::new().width(Length::Fill),
            ]
            .padding(20),
            row![
                container(tabs).padding(10).style(theme::sidebar_style),
                container(scrollable(content).style(theme::scrollable_style))
                    .padding(20)
                    .width(Length::Fill),
            ]
            .height(Length::Fill),
            container(footer).padding(10).style(theme::footer_style),
        ])
        .width(modal_width)
        .height(modal_height)
        .padding(padding_outer)
        .style(theme::modal_container)
        .into()
    }
}

// Helpers que aceptan Strings y hacen to_string() para ser 'static
fn tab_button(
    label: &str,
    icon_data: &'static str,
    tab: Tab,
    current: &Tab,
    compact: bool,
) -> Element<'static, SettingsMessage> {
    let is_active = tab == *current;

    let content: Element<'static, SettingsMessage> = if compact {
        // Solo icono, centrado
        row![
            svg(util::icons::icon(icon_data))
                .width(20)
                .height(20)
                .style(theme::svg_accent),
        ]
        .align_y(Alignment::Center)
        .padding(5)
        .into()
    } else {
        // Icono + Texto
        row![
            svg(util::icons::icon(icon_data))
                .width(16)
                .height(16)
                .style(theme::svg_accent),
            text(label.to_string()).size(14)
        ]
        .spacing(10)
        .align_y(Alignment::Center)
        .into()
    };

    button(
        container(content)
            .width(Length::Fill)
            .center_x(Length::Fill)
            .padding(10),
    )
    .on_press(SettingsMessage::TabSelected(tab))
    .width(Length::Fill)
    .style(if is_active {
        theme::active_tab_style
    } else {
        theme::ghost_button_style
    })
    .into()
}

fn section_title(label: impl Into<String>) -> Element<'static, SettingsMessage> {
    text(label.into())
        .size(16)
        .color(theme::ACCENT_ORANGE)
        .into()
}

fn input_group(
    label: impl Into<String>,
    value: &str,
    on_change: fn(String) -> SettingsMessage,
) -> Element<'static, SettingsMessage> {
    column![
        text(label.into())
            .size(12)
            .color(Color::from_rgb(0.7, 0.7, 0.7)),
        text_input("", value)
            .on_input(on_change)
            .padding(8)
            .style(theme::text_input_style)
            .width(100)
    ]
    .spacing(5)
    .into()
}

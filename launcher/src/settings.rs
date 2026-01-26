use crate::Message;
use crate::config::GameSettings;
use crate::{theme, util};
use iced::widget::{
    Space, button, checkbox, column, container, pick_list, row, scrollable, slider, svg, text,
    text_input,
};
use iced::{Alignment, Color, Element, Length, Size};

#[derive(Debug, Clone, PartialEq)]
pub enum ThemePreset {
    Rustale,
    Crimson,
    Emerald,
    Sky,
    Amethyst,
    Custom,
}

impl ThemePreset {
    pub fn color(&self) -> &'static str {
        match self {
            Self::Rustale => "#FFA845",
            Self::Crimson => "#FF4545",
            Self::Emerald => "#45FF88",
            Self::Sky => "#45A8FF",
            Self::Amethyst => "#A845FF",
            Self::Custom => "",
        }
    }

    pub fn all() -> Vec<ThemePreset> {
        vec![
            Self::Rustale,
            Self::Crimson,
            Self::Emerald,
            Self::Sky,
            Self::Amethyst,
            Self::Custom,
        ]
    }
}

impl std::fmt::Display for ThemePreset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Tab {
    Launcher,
    Game,
    Video,
    Java,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UpdateStatus {
    Idle,
    Checking,
    Found(crate::updater::ReleaseInfo),
    UpToDate,
    Error(String),
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
    RepairVersion(u32),
    OpenVersionFolder(u32),

    PickMoveLocation,
    PerformMove(std::path::PathBuf),
    OpenCurrentDataDir,
    LanguageSelected(crate::lang::Language),
    OnlineFixModeChanged(crate::config::OnlineFixMode),
    ToggleMinimizeTray(bool),
    ToggleMinimizePlay(bool),
    QuickplayToggled(bool),
    AutoUpdateToggled(bool),
    CheckForLauncherUpdates,
    UpdateResult(Result<Option<crate::updater::ReleaseInfo>, String>),
    ResetUpdateStatus,
    WaitAndReset,
    // Theme
    ThemePresetSelected(ThemePreset),
    ThemeHexChanged(String),
    ThemeSaturationChanged(f32),
    ThemeContrastChanged(f32),
    LsdToggled(bool),
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
    pub update_btn_status: UpdateStatus,
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
            update_btn_status: UpdateStatus::Idle,
        }
    }

    pub fn open(&mut self, settings: GameSettings) {
        self.temp_settings = settings;
        self.new_install_path = crate::config::get_app_dir().to_string_lossy().to_string();
        self.current_tab = Tab::Launcher; // Opcional: volver siempre a la primera pestana
        self.is_open = true;
        self.update_btn_status = UpdateStatus::Idle;
        // installed_versions should be updated via message shortly after opening
    }

    fn view_launcher_tab<'a>(
        &'a self,
        localization: &'a crate::lang::Localization,
        _is_compact: bool, // Recibimos el parámetro
        ctx: theme::UIContext,
    ) -> Element<'a, SettingsMessage> {
        let palette = ctx.palette;
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
        .placeholder(localization.t("settings.language_placeholder"))
        .padding(10)
        .width(150)
        .style(move |t, status| theme::pick_list_style(&palette, t, status))
        .menu_style(move |t| theme::menu_style(&palette, t));

        let news_checkbox = row![
            checkbox(self.temp_settings.enable_news)
                .on_toggle(SettingsMessage::EnableNewsToggled)
                .style(move |t, s| theme::checkbox_style(&palette, t, s)),
            theme::text(text(localization.t("settings.enable_news")).size(14), ctx),
        ]
        .spacing(10)
        .align_y(Alignment::Center);

        let minimize_tray_chk = row![
            checkbox(self.temp_settings.minimize_to_tray)
                .on_toggle(SettingsMessage::ToggleMinimizeTray)
                .style(move |t, s| theme::checkbox_style(&palette, t, s)),
            theme::text(
                text(localization.t("settings.minimize_to_tray")).size(14),
                ctx
            )
        ]
        .spacing(10)
        .align_y(Alignment::Center);

        let minimize_play_chk = row![
            checkbox(self.temp_settings.minimize_on_play)
                .on_toggle(SettingsMessage::ToggleMinimizePlay)
                .style(move |t, s| theme::checkbox_style(&palette, t, s)),
            theme::text(
                text(localization.t("settings.minimize_on_play")).size(14),
                ctx
            )
        ]
        .spacing(10)
        .align_y(Alignment::Center);

        let auto_update_chk = row![
            checkbox(self.temp_settings.enable_auto_update)
                .on_toggle(SettingsMessage::AutoUpdateToggled)
                .style(move |t, s| theme::checkbox_style(&palette, t, s)),
            theme::text(text(localization.t("settings.auto_update")).size(14), ctx),
        ]
        .spacing(10)
        .align_y(Alignment::Center);

        let quickplay_chk = row![
            checkbox(self.temp_settings.quickplay)
                .on_toggle(SettingsMessage::QuickplayToggled)
                .style(move |t, s| theme::checkbox_style(&palette, t, s)),
            column![
                theme::text(text(localization.t("settings.quickplay")).size(14), ctx),
                theme::text(
                    text(localization.t("settings.quickplay_desc"))
                        .size(10)
                        .color(Color::from_rgb(0.5, 0.5, 0.5)),
                    ctx
                ),
            ]
        ]
        .spacing(10)
        .align_y(Alignment::Center);

        // --- NUEVO: Tema Dinámico ---
        let presets = ThemePreset::all();
        let current_preset = presets
            .iter()
            .find(|p| p.color() == self.temp_settings.theme.accent_hex)
            .cloned()
            .unwrap_or(ThemePreset::Custom);

        let theme_section = column![
            section_title(localization.t("settings.theme"), ctx),
            row![
                theme::text(
                    text(localization.t("settings.theme_preset"))
                        .size(12)
                        .width(100),
                    ctx
                ),
                pick_list(
                    presets,
                    Some(current_preset),
                    SettingsMessage::ThemePresetSelected
                )
                .width(150)
                .style(move |t, s| theme::pick_list_style(&palette, t, s))
                .menu_style(move |t| theme::menu_style(&palette, t))
            ]
            .spacing(10)
            .align_y(Alignment::Center),
            row![
                theme::text(text("Hex").size(12).width(100), ctx),
                text_input("#RRGGBB", &self.temp_settings.theme.accent_hex)
                    .on_input(SettingsMessage::ThemeHexChanged)
                    .width(100)
                    .style(move |t, s| theme::text_input_style(&palette, t, s))
            ]
            .spacing(10)
            .align_y(Alignment::Center),
            // Saturación
            column![
                theme::text(text(localization.t("settings.saturation")).size(12), ctx),
                slider(
                    0.0..=2.0,
                    self.temp_settings.theme.saturation,
                    SettingsMessage::ThemeSaturationChanged
                )
                .step(0.1)
                .style(move |t, s| theme::slider_style(&palette, t, s))
            ]
            .spacing(5),
            // Contraste
            column![
                theme::text(text(localization.t("settings.contrast")).size(12), ctx),
                slider(
                    0.5..=1.5,
                    self.temp_settings.theme.contrast,
                    SettingsMessage::ThemeContrastChanged
                )
                .step(0.1)
                .style(move |t, s| theme::slider_style(&palette, t, s))
            ]
            .spacing(5),
        ]
        .spacing(10);

        column![
            section_title(localization.t("settings.tabs.launcher"), ctx),
            column![
                theme::text(
                    text(localization.t("settings.language"))
                        .size(12)
                        .color(Color::from_rgb(0.7, 0.7, 0.7)),
                    ctx
                ),
                language_pick
            ]
            .spacing(5),
            Space::new().height(10),
            theme_section,
            Space::new().height(10),
            news_checkbox,
            auto_update_chk,
            self.view_update_check_button(localization, _is_compact, ctx),
            minimize_tray_chk,
            minimize_play_chk,
            quickplay_chk,
            row![
                checkbox(self.temp_settings.theme.lsd_mode)
                    .on_toggle(SettingsMessage::LsdToggled)
                    .style(move |t, s| theme::checkbox_style(&palette, t, s)),
                column![
                    theme::text(text("LSD").size(14).color(palette.accent), ctx),
                    theme::text(
                        text("Experience the launcher in another dimension.")
                            .size(10)
                            .color(palette.text_secondary),
                        ctx
                    ),
                ]
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        ]
        .spacing(15)
        .width(Length::Fill)
        .into()
    }

    fn view_update_check_button<'a>(
        &'a self,
        localization: &'a crate::lang::Localization,
        is_compact: bool,
        ctx: theme::UIContext,
    ) -> Element<'a, SettingsMessage> {
        let palette = ctx.palette;
        let content = match &self.update_btn_status {
            UpdateStatus::Idle => localization.t("settings.check_updates").to_string(),
            UpdateStatus::Checking => localization.t("launcher.status.checking").to_string(),
            UpdateStatus::Found(info) => {
                format!("{} v{}", localization.t("mods.install"), info.tag_name)
            }
            UpdateStatus::UpToDate => localization.t("launcher.status.ready").to_string(),
            UpdateStatus::Error(_) => localization.t("settings.check_failed").to_string(),
        };

        let style: fn(
            &theme::Palette,
            &iced::Theme,
            iced::widget::button::Status,
        ) -> iced::widget::button::Style = match &self.update_btn_status {
            UpdateStatus::Found(_) => theme::primary_button_style,
            UpdateStatus::Error(_) => theme::danger_button_style,
            _ => theme::secondary_button_style,
        };

        let mut btn = button(
            container(
                row![
                    theme::svg(
                        svg(util::icons::icon(util::icons::REFRESH))
                            .width(14)
                            .height(14)
                            .style(move |t, s| theme::svg_accent(&palette, t, s)),
                        ctx
                    ),
                    theme::text(text(content).size(14), ctx)
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            )
            .center_x(Length::Fill),
        );

        if !matches!(self.update_btn_status, UpdateStatus::Checking) {
            btn = btn.on_press(SettingsMessage::CheckForLauncherUpdates);
        }

        btn.style(move |t, s| style(&palette, t, s))
            .width(if is_compact {
                Length::Fill
            } else {
                Length::Fixed(180.0)
            })
            .padding(8)
            .into()
    }

    fn view_game_tab<'a>(
        &'a self,
        localization: &'a crate::lang::Localization,
        is_compact: bool,
        ctx: theme::UIContext,
    ) -> Element<'a, SettingsMessage> {
        let palette = ctx.palette;
        // --- 1. Installation Path Selector ---
        let current_path_display = crate::config::get_app_dir().to_string_lossy().to_string();

        let path_selector = column![
            section_title(localization.t("settings.install_path"), ctx),
            row![
                text_input(
                    localization.t("settings.game.path_field"),
                    &current_path_display
                )
                .size(14)
                .style(move |t, s| theme::text_input_style(&palette, t, s))
                .width(Length::Fill),
                button(
                    row![
                        theme::svg(
                            svg(util::icons::icon(util::icons::FOLDER))
                                .width(14)
                                .height(14)
                                .style(move |t, s| theme::svg_accent(&palette, t, s)),
                            ctx
                        ),
                        theme::text(text("Open").size(14), ctx)
                    ]
                    .spacing(5)
                    .align_y(Alignment::Center)
                )
                .on_press(SettingsMessage::OpenCurrentDataDir)
                .style(move |t, s| theme::secondary_button_style(&palette, t, s))
                .padding(10),
                button(theme::text(text("Move to...").size(14), ctx))
                    .on_press(SettingsMessage::PickMoveLocation)
                    .style(move |t, s| theme::primary_button_style(&palette, t, s))
                    .padding(10),
                Space::new().width(10)
            ]
            .spacing(10)
            .align_y(Alignment::Center)
        ]
        .spacing(5);

        // --- 2. Channel & Version Pickers ---
        // Create "Channel Section" as a typed Element to resolve inference immediately
        let channel_section: Element<'a, SettingsMessage> = column![
            theme::text(
                text(localization.t("settings.update_channel"))
                    .size(12)
                    .color(Color::from_rgb(0.7, 0.7, 0.7)),
                ctx
            ),
            pick_list(
                vec!["release", "pre-release"],
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
            .style(move |t, s| theme::pick_list_style(&palette, t, s))
            .menu_style(move |t| theme::menu_style(&palette, t))
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
            theme::text(
                text(localization.t("settings.target_version"))
                    .size(12)
                    .color(Color::from_rgb(0.7, 0.7, 0.7)),
                ctx
            ),
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
            .style(move |t, s| theme::pick_list_style(&palette, t, s))
            .menu_style(move |t| theme::menu_style(&palette, t))
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
        .style(move |t, status| theme::pick_list_style(&palette, t, status))
        .menu_style(move |t| theme::menu_style(&palette, t));

        let online_fix_checkbox = row![
            checkbox(self.temp_settings.enable_online_fix)
                .on_toggle(SettingsMessage::EnableOnlineFixToggled)
                .style(move |t, s| theme::checkbox_style(&palette, t, s)),
            column![
                theme::text(
                    text(localization.t("settings.enable_online_fix")).size(14),
                    ctx
                ),
                theme::text(
                    text(localization.t("settings.online_fix_desc"))
                        .size(10)
                        .color(Color::from_rgb(0.5, 0.5, 0.5)),
                    ctx
                ),
            ]
        ]
        .spacing(10)
        .align_y(Alignment::Center);

        let online_fix_section = column![
            online_fix_checkbox,
            if self.temp_settings.enable_online_fix {
                container(
                    row![
                        theme::text(
                            text(localization.t("settings.patch_mode"))
                                .size(12)
                                .color(Color::from_rgb(0.7, 0.7, 0.7)),
                            ctx
                        ),
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
            section_title(localization.t("settings.game_config"), ctx),
            online_fix_section,
            Space::new().height(5),
            version_controls,
            Space::new().height(5),
        ]
        .spacing(10);

        // --- 3. Storage Management (Installed Versions) ---
        let installed_content: Element<'_, SettingsMessage> = if self.installed_versions.is_empty()
        {
            container(theme::text(
                text(localization.t("settings.storage.no_versions"))
                    .size(12)
                    .color(Color::from_rgb(0.5, 0.5, 0.5)),
                ctx,
            ))
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
                            theme::text(text(label).size(12).width(Length::Fill), ctx),
                            button(
                                container(theme::svg(
                                    svg(util::icons::icon(util::icons::FOLDER))
                                        .style(move |t, s| theme::svg_accent(&palette, t, s)),
                                    ctx
                                ),)
                                .center_x(Length::Fill)
                                .center_y(Length::Fill)
                            )
                            .on_press(SettingsMessage::OpenVersionFolder(version_val))
                            .style(move |t, s| theme::secondary_button_style(&palette, t, s)),
                            Space::new().width(5),
                            button(
                                container(theme::svg(
                                    svg(util::icons::icon(util::icons::WRENCH))
                                        .style(move |t, s| theme::svg_accent(&palette, t, s)),
                                    ctx
                                ),)
                                .center_x(Length::Fill)
                                .center_y(Length::Fill)
                            )
                            .on_press(SettingsMessage::RepairVersion(version_val))
                            .style(move |t, s| theme::secondary_button_style(&palette, t, s)),
                            Space::new().width(5),
                            button(
                                container(theme::svg(
                                    svg(util::icons::icon(util::icons::TRASH))
                                        .style(move |t, s| theme::svg_accent(&palette, t, s)),
                                    ctx
                                ),)
                                .center_x(Length::Fill)
                                .center_y(Length::Fill)
                            )
                            .on_press(SettingsMessage::DeleteVersion(version_val))
                            .style(move |t, s| theme::secondary_button_style(&palette, t, s)),
                        ]
                        .align_y(Alignment::Center),
                    )
                    .padding(5)
                    .style(move |t| theme::sidebar_style(&palette, t))
                    .width(Length::Fill)
                    .height(40),
                );
            }
            container(
                scrollable(list)
                    .height(120)
                    .style(move |t, s| theme::scrollable_style(&palette, t, s)),
            )
            .padding(iced::Padding {
                top: 0.0,
                right: 10.0,
                bottom: 0.0,
                left: 0.0,
            })
            .into()
        };

        let installed_manager = column![
            section_title(localization.t("settings.storage.title"), ctx),
            theme::text(
                text(localization.t("settings.storage.desc"))
                    .size(12)
                    .color(Color::from_rgb(0.5, 0.5, 0.5)),
                ctx
            ),
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
                None
            }
            SettingsMessage::CloseModal => {
                self.is_open = false;
                Some(Message::CloseSettings)
            }
            SettingsMessage::SaveSettings => {
                self.is_open = false;
                Some(Message::SaveSettings(self.temp_settings.clone()))
            }
            SettingsMessage::PickMoveLocation => {
                Some(Message::Settings(SettingsMessage::PickMoveLocation))
            }
            SettingsMessage::OpenCurrentDataDir => {
                Some(Message::Settings(SettingsMessage::OpenCurrentDataDir))
            }
            SettingsMessage::PerformMove(_path) => None,
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
            SettingsMessage::RepairVersion(v) => Some(Message::RequestRepairVersion(v)),
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
            SettingsMessage::CheckForLauncherUpdates => {
                if let UpdateStatus::Found(info) = &self.update_btn_status {
                    if let Some(url) = crate::updater::get_asset_url(info) {
                        return Some(Message::LauncherUpdate(
                            crate::updater::UpdaterMessage::StartUpdate(url),
                        ));
                    }
                }

                self.update_btn_status = UpdateStatus::Checking;
                Some(Message::LauncherUpdate(
                    crate::updater::UpdaterMessage::CheckForUpdates,
                ))
            }
            SettingsMessage::UpdateResult(res) => {
                match res {
                    Ok(Some(info)) => {
                        self.update_btn_status = UpdateStatus::Found(info);
                    }
                    Ok(None) => {
                        self.update_btn_status = UpdateStatus::UpToDate;
                        return Some(Message::Settings(SettingsMessage::WaitAndReset));
                    }
                    Err(e) => {
                        self.update_btn_status = UpdateStatus::Error(e);
                        return Some(Message::Settings(SettingsMessage::WaitAndReset));
                    }
                }
                None
            }
            SettingsMessage::ResetUpdateStatus => {
                if !matches!(self.update_btn_status, UpdateStatus::Found(_)) {
                    self.update_btn_status = UpdateStatus::Idle;
                }
                None
            }
            SettingsMessage::WaitAndReset => None,
            SettingsMessage::ThemePresetSelected(preset) => {
                if let ThemePreset::Custom = preset {
                    // Do nothing, let them edit manually
                } else {
                    self.temp_settings.theme.accent_hex = preset.color().to_string();
                    self.temp_settings.theme.saturation = 1.0;
                    self.temp_settings.theme.contrast = 1.0;
                }
                None
            }
            SettingsMessage::ThemeHexChanged(val) => {
                if val.len() <= 7 {
                    self.temp_settings.theme.accent_hex = val;
                }
                None
            }
            SettingsMessage::ThemeSaturationChanged(val) => {
                self.temp_settings.theme.saturation = val;
                None
            }
            SettingsMessage::ThemeContrastChanged(val) => {
                self.temp_settings.theme.contrast = val;
                None
            }
            SettingsMessage::LsdToggled(val) => {
                self.temp_settings.theme.lsd_mode = val;
                None
            }
            SettingsMessage::None => None,
        }
    }

    pub fn view<'a>(
        &'a self,
        localization: &'a crate::lang::Localization,
        window_size: Size,
        ctx: theme::UIContext,
    ) -> Element<'a, SettingsMessage> {
        let palette = ctx.palette;
        // Detectar modo compacto
        let is_compact = window_size.width < 650.0;
        let tab_width = if is_compact { 60.0 } else { 170.0 };

        let tabs = column![
            tab_button(
                &localization.t("settings.tabs.launcher"),
                util::icons::SETTINGS,
                Tab::Launcher,
                &self.current_tab,
                is_compact,
                ctx,
            ),
            tab_button(
                &localization.t("settings.tabs.game"),
                util::icons::GAMEPAD,
                Tab::Game,
                &self.current_tab,
                is_compact,
                ctx,
            ),
            tab_button(
                &localization.t("settings.tabs.video"),
                util::icons::MONITOR,
                Tab::Video,
                &self.current_tab,
                is_compact,
                ctx,
            ),
            tab_button(
                &localization.t("settings.tabs.java"),
                util::icons::COFFEE,
                Tab::Java,
                &self.current_tab,
                is_compact,
                ctx,
            )
        ]
        .spacing(5)
        .width(tab_width);

        let content = match self.current_tab {
            Tab::Launcher => self.view_launcher_tab(localization, is_compact, ctx),
            Tab::Game => self.view_game_tab(localization, is_compact, ctx),
            Tab::Video => column![
                section_title(localization.t("settings.display"), ctx),
                row![
                    input_group(
                        localization.t("settings.width"),
                        &self.temp_settings.width.to_string(),
                        SettingsMessage::WidthChanged,
                        ctx
                    ),
                    input_group(
                        localization.t("settings.height"),
                        &self.temp_settings.height.to_string(),
                        SettingsMessage::HeightChanged,
                        ctx
                    ),
                ]
                .spacing(20),
                Space::new().height(20),
                row![
                    checkbox(self.temp_settings.fullscreen)
                        .on_toggle(SettingsMessage::FullscreenToggled)
                        .style(move |t, s| theme::checkbox_style(&palette, t, s)),
                    theme::text(text(localization.t("settings.fullscreen")).size(14), ctx)
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            ]
            .spacing(15)
            .into(),
            Tab::Java => column![
                section_title(localization.t("settings.java_memory"), ctx),
                theme::text(
                    text(format!(
                        "{}: {} GB",
                        localization.t("settings.min"),
                        self.temp_settings.min_memory
                    ))
                    .size(12),
                    ctx
                ),
                slider(
                    1.0..=16.0,
                    self.temp_settings.min_memory as f32,
                    SettingsMessage::MinMemoryChanged
                )
                .step(1.0)
                .style(move |t, s| theme::slider_style(&palette, t, s)),
                Space::new().height(10),
                theme::text(
                    text(format!(
                        "{}: {} GB",
                        localization.t("settings.max"),
                        self.temp_settings.max_memory
                    ))
                    .size(12),
                    ctx
                ),
                slider(
                    1.0..=32.0,
                    self.temp_settings.max_memory as f32,
                    SettingsMessage::MaxMemoryChanged
                )
                .step(1.0)
                .style(move |t, s| theme::slider_style(&palette, t, s)),
                Space::new().height(20),
                section_title(localization.t("settings.jvm_args"), ctx),
                text_input(
                    localization.t("settings.jvm_args_placeholder"),
                    &self.temp_settings.java_args
                )
                .on_input(SettingsMessage::JavaArgsChanged)
                .padding(10)
                .style(move |t, status| theme::text_input_style(&palette, t, status)),
            ]
            .spacing(15)
            .into(),
        };

        let footer = row![
            button(theme::text(
                text(localization.t("settings.cancel")).size(14),
                ctx
            ))
            .on_press(SettingsMessage::CloseModal)
            .style(move |t, s| theme::secondary_button_style(&palette, t, s))
            .padding(10), // Reduced visible scaling by keeping padding reasonable but text small
            Space::new().width(10),
            button(theme::text(
                text(localization.t("settings.save")).size(14),
                ctx
            ))
            .on_press(SettingsMessage::SaveSettings)
            .style(move |t, status| theme::primary_button_style(&palette, t, status))
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
                theme::text(
                    text(localization.t("settings.title"))
                        .size(18)
                        .font(iced::font::Font::MONOSPACE),
                    ctx
                ),
                Space::new().width(Length::Fill),
            ]
            .padding(20),
            row![
                container(tabs)
                    .padding(10)
                    .style(move |t| theme::sidebar_style(&palette, t)),
                container(
                    scrollable(content)
                        .style(move |t, status| theme::scrollable_style(&palette, t, status))
                )
                .padding(20)
                .width(Length::Fill),
            ]
            .height(Length::Fill),
            container(footer)
                .padding(10)
                .style(move |t| theme::footer_style(&palette, t)),
        ])
        .width(modal_width)
        .height(modal_height)
        .padding(padding_outer)
        .style(move |t| theme::modal_container(&palette, t))
        .into()
    }
}

// Helpers que aceptan Strings y hacen to_string() para ser 'static
fn tab_button<'a>(
    label: &str,
    icon_data: &'static str,
    tab: Tab,
    current: &Tab,
    compact: bool,
    ctx: theme::UIContext,
) -> Element<'a, SettingsMessage> {
    let palette = ctx.palette;
    let is_active = tab == *current;

    let content: Element<'a, SettingsMessage> = if compact {
        // Solo icono, centrado
        row![theme::svg(
            svg(util::icons::icon(icon_data))
                .width(16)
                .height(16)
                .style(move |t, status| theme::svg_accent(&palette, t, status)),
            ctx
        ),]
        .align_y(Alignment::Center)
        .padding(0)
        .into()
    } else {
        // Icono + Texto
        row![
            theme::svg(
                svg(util::icons::icon(icon_data))
                    .width(16)
                    .height(16)
                    .style(move |t, status| theme::svg_accent(&palette, t, status)),
                ctx
            ),
            theme::text(text(label.to_string()).size(14), ctx)
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
    .style(move |t, status| {
        if is_active {
            theme::active_tab_style(&palette, t, status)
        } else {
            theme::ghost_button_style(&palette, t, status)
        }
    })
    .into()
}

fn section_title<'a>(
    label: impl Into<String>,
    ctx: theme::UIContext,
) -> Element<'a, SettingsMessage> {
    let palette = ctx.palette;
    theme::text(text(label.into()).size(16).color(palette.accent), ctx).into()
}

fn input_group<'a>(
    label: impl Into<String>,
    value: &str,
    on_change: fn(String) -> SettingsMessage,
    ctx: theme::UIContext,
) -> Element<'a, SettingsMessage> {
    let palette = ctx.palette;
    column![
        theme::text(
            text(label.into())
                .size(12)
                .color(Color::from_rgb(0.7, 0.7, 0.7)),
            ctx
        ),
        text_input("", value)
            .on_input(on_change)
            .padding(8)
            .style(move |t, status| theme::text_input_style(&palette, t, status))
            .width(100)
    ]
    .spacing(5)
    .into()
}

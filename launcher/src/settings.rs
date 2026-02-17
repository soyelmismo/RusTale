use crate::Message;
use crate::config::GameSettings;
use crate::{theme, util};
use iced::widget::{
    Id, Space, button, checkbox, column, container, pick_list, row, scrollable, slider, svg,
    text_input,
};
use iced::{Alignment, Element, Length, Size};

// ID estatico para el area de scroll
const SETTINGS_SCROLL_ID: &str = "settings_scroll_area";

const CHANNELS: &[&str] = &["release", "pre-release"];

const ONLINE_FIX_MODES: &[crate::config::OnlineFixMode] = &[
    crate::config::OnlineFixMode::Local,
    crate::config::OnlineFixMode::Sanasol,
];

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
    ScaleFactorChanged(f32),
    FullscreenToggled(bool),
    // Java
    MinMemoryChanged(f32),
    MaxMemoryChanged(f32),
    JavaArgsChanged(String),
    JavaArgsAction(iced::widget::text_editor::Action),
    JavaInfoLoaded,
    JavaVersionUpdated(String),
    JavaLoadingStarted,

    CloseModal,
    SaveSettings,

    VersionSelected(u32),
    DeleteVersion(u32),
    RepairVersion(u32),
    OpenVersionFolder(u32),

    PickMoveLocation,
    PickUseDataLocation,
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
    LsdHovered(bool),
    BaseThemeChanged(crate::config::BaseThemeMode),

    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionOption {
    Latest,
    Specific(u32),
}

impl std::fmt::Display for VersionOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // En este impl no tenemos acceso a la Localization facilmente sin cambiar muchas firmas.
        // Pero como se usa principalmente en PickList, el PickList puede usar una funcion de mapeo manual.
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
#[derive(Debug, Clone)]
pub struct SettingsState {
    pub current_tab: Tab,
    pub temp_settings: GameSettings,
    pub is_open: bool,
    pub available_versions: Vec<i32>,
    pub installed_versions: Vec<(i32, bool)>, // (version, is_latest_folder)
    pub is_loading_versions: bool,
    pub new_install_path: String,
    pub update_btn_status: UpdateStatus,
    pub java_loading: bool,
    pub java_info_loaded: bool,
    pub java_version: Option<String>,
    pub jvm_args_content: iced::widget::text_editor::Content,
}

impl SettingsState {
    pub fn new(current_settings: &GameSettings) -> Self {
        let initial_java_args = current_settings.java_args.clone();

        Self {
            current_tab: Tab::Game,
            temp_settings: current_settings.clone(),
            available_versions: Vec::new(),
            installed_versions: Vec::new(),
            is_open: false,
            is_loading_versions: false,
            new_install_path: String::new(),
            update_btn_status: UpdateStatus::Idle,
            java_loading: false,
            java_info_loaded: false,
            java_version: None,
            // Usamos la variable clonada
            jvm_args_content: iced::widget::text_editor::Content::with_text(&initial_java_args),
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
        is_compact: bool, // Recibimos el parametro
        ctx: theme::UIContext,
    ) -> Element<'a, SettingsMessage> {
        let palette = ctx.palette;
        let news_checkbox = row![
            theme::magic_checkbox(
                checkbox(self.temp_settings.enable_news)
                    .on_toggle(SettingsMessage::EnableNewsToggled)
                    .style(move |t, s| theme::checkbox_style(&palette, t, s))
                    .into(),
                ctx
            ),
            theme::text_body(localization.t("settings.enable_news"), ctx),
        ]
        .spacing(10)
        .align_y(Alignment::Center);

        let minimize_tray_text = if cfg!(target_os = "linux") {
            localization.t("settings.minimize_to_tray_linux")
        } else {
            localization.t("settings.minimize_to_tray")
        };

        let minimize_tray_chk = row![
            theme::magic_checkbox(
                checkbox(self.temp_settings.minimize_to_tray)
                    .on_toggle(SettingsMessage::ToggleMinimizeTray)
                    .style(move |t, s| theme::checkbox_style(&palette, t, s))
                    .into(),
                ctx
            ),
            theme::text_body(minimize_tray_text, ctx)
        ]
        .spacing(10)
        .align_y(Alignment::Center);

        let minimize_play_chk = row![
            theme::magic_checkbox(
                checkbox(self.temp_settings.minimize_on_play)
                    .on_toggle(SettingsMessage::ToggleMinimizePlay)
                    .style(move |t, s| theme::checkbox_style(&palette, t, s))
                    .into(),
                ctx
            ),
            theme::text_body(localization.t("settings.minimize_on_play"), ctx)
        ]
        .spacing(10)
        .align_y(Alignment::Center);

        let auto_update_chk = row![
            theme::magic_checkbox(
                checkbox(self.temp_settings.enable_auto_update)
                    .on_toggle(SettingsMessage::AutoUpdateToggled)
                    .style(move |t, s| theme::checkbox_style(&palette, t, s))
                    .into(),
                ctx
            ),
            theme::text_body(localization.t("settings.auto_update"), ctx),
        ]
        .spacing(10)
        .align_y(Alignment::Center);

        let quickplay_chk = row![
            theme::magic_checkbox(
                checkbox(self.temp_settings.quickplay)
                    .on_toggle(SettingsMessage::QuickplayToggled)
                    .style(move |t, s| theme::checkbox_style(&palette, t, s))
                    .into(),
                ctx
            ),
            column![
                theme::text_body(localization.t("settings.quickplay"), ctx),
                theme::text_caption(localization.t("settings.quickplay_desc"), ctx),
            ]
        ]
        .spacing(10)
        .align_y(Alignment::Center);

        let theme_presets = ThemePreset::all();
        let selected_preset = theme_presets
            .iter()
            .find(|p| {
                self.temp_settings.theme.accent_hex == p.color()
                    || (p.color().is_empty() && self.temp_settings.theme.accent_hex.is_empty())
            })
            .cloned();

        let theme_preset_pick = theme::magic_pick_list_with_menu(
            pick_list(
                theme_presets.clone(),
                selected_preset,
                SettingsMessage::ThemePresetSelected,
            )
            .text_size(14)
            .placeholder(localization.t("settings.theme_preset"))
            .padding(10)
            .width(150)
            .style(move |t, status| theme::pick_list_style(&palette, t, status))
            .menu_style(move |t| theme::menu_style(&palette, t))
            .into(),
            ctx,
        );

        let theme_section = column![
            theme::text_title(localization.t("settings.theme"), ctx),
            row![
                theme_mode_button(
                    localization.t("settings.theme_mode_dark"),
                    crate::config::BaseThemeMode::Black,
                    self.temp_settings.theme.base_mode,
                    ctx
                ),
                theme_mode_button(
                    localization.t("settings.theme_mode_gray"),
                    crate::config::BaseThemeMode::Grey,
                    self.temp_settings.theme.base_mode,
                    ctx
                ),
                theme_mode_button(
                    localization.t("settings.theme_mode_light"),
                    crate::config::BaseThemeMode::Light,
                    self.temp_settings.theme.base_mode,
                    ctx
                ),
            ]
            .spacing(2)
            .width(Length::Fill),
            // Theme Preset Selector
            row![
                theme::text_small(localization.t("settings.theme_preset"), ctx),
                theme_preset_pick
            ]
            .spacing(10)
            .align_y(Alignment::Center),
            row![
                theme::text_small("Hex", ctx),
                theme::magic_text_input(
                    text_input("#RRGGBB", &self.temp_settings.theme.accent_hex)
                        .on_input(SettingsMessage::ThemeHexChanged)
                        .width(100)
                        .style(move |t, s| theme::text_input_style(&palette, t, s))
                        .into(),
                    ctx,
                )
            ]
            .spacing(10)
            .align_y(Alignment::Center),
            // Saturacion
            column![
                theme::text_small(localization.t("settings.saturation"), ctx),
                theme::magic_slider(
                    slider(
                        0.0..=2.0,
                        self.temp_settings.theme.saturation,
                        SettingsMessage::ThemeSaturationChanged
                    )
                    .step(0.1)
                    .style(move |t, s| theme::slider_style(&palette, t, s))
                    .into(),
                    ctx
                )
            ]
            .spacing(5),
            // Contraste
            column![
                theme::text_small(localization.t("settings.contrast"), ctx),
                theme::magic_slider(
                    slider(
                        0.5..=1.5,
                        self.temp_settings.theme.contrast,
                        SettingsMessage::ThemeContrastChanged
                    )
                    .step(0.1)
                    .style(move |t, s| theme::slider_style(&palette, t, s))
                    .into(),
                    ctx
                )
            ]
            .spacing(5),
        ]
        .spacing(10);

        let selected_language = localization
            .available_languages
            .iter()
            .find(|l| l.id == self.temp_settings.language)
            .cloned();

        // --- CORRECCIoN 2: Envolver el retorno en una tupla ( ) explicita ---
        let language_pick = theme::magic_pick_list_with_menu(
            pick_list(
                &localization.available_languages[..],
                selected_language,
                SettingsMessage::LanguageSelected,
            )
            .text_size(14)
            .placeholder(localization.t("settings.language_placeholder"))
            .padding(10)
            .width(150)
            .style(move |t, status| theme::pick_list_style(&palette, t, status))
            .menu_style(move |t| theme::menu_style(&palette, t))
            .into(),
            ctx,
        );
        theme::page_container(theme::magic_column(
            vec![
                section_title(localization.t("settings.tabs.launcher"), ctx),
                column![
                    theme::text_small(localization.t("settings.language"), ctx),
                    language_pick
                ]
                .spacing(5)
                .into(),
                Space::new().height(10).into(),
                theme_section.into(),
                Space::new().height(10).into(),
                news_checkbox.into(),
                auto_update_chk.into(),
                self.view_update_check_button(localization, is_compact, ctx),
                minimize_tray_chk.into(),
                minimize_play_chk.into(),
                quickplay_chk.into(),
                // === SOLUCION ANTI-BUCLE INFINITO ===
                {
                    let text_content = column![
                        theme::lsd_magic_text("LSD", ctx),
                        theme::text_muted(localization.t("settings.lsd_desc"), ctx),
                    ]
                    .spacing(2)
                    .width(Length::Fixed(180.0)); // Ancho fijo para que no empuje al checkbox

                    // 3. La fila organiza los elementos de forma independiente
                    row![
                        // El checkbox esta completamente aislado
                        theme::magic_checkbox(
                            checkbox(self.temp_settings.theme.lsd_mode)
                                .on_toggle(SettingsMessage::LsdToggled)
                                .style(move |t, s| theme::checkbox_style(&palette, t, s))
                                .into(),
                            ctx
                        ),
                        text_content
                    ]
                    .spacing(15) // Espaciado generoso para evitar interferencias
                    .align_y(Alignment::Center)
                    .into()
                },
            ],
            ctx,
        ))
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
                    theme::text_body(content, ctx)
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            )
            .center_x(Length::Fill)
            .style(move |t| theme::container_style_transparent(&palette, t)),
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
                button(
                    row![theme::text_body(current_path_display.clone(), ctx)]
                        .align_y(Alignment::Center)
                )
                .on_press(SettingsMessage::OpenCurrentDataDir)
                .style(move |t, s| theme::secondary_button_style(&palette, t, s))
                .width(Length::Fill)
                .padding([10, 12]),
                button(
                    row![
                        theme::svg(
                            svg(util::icons::icon(util::icons::FOLDER))
                                .width(14)
                                .height(14)
                                .style(move |t, s| theme::svg_accent(&palette, t, s)),
                            ctx
                        ),
                        theme::text_body(localization.t("settings.open_folder"), ctx)
                    ]
                    .spacing(5)
                    .align_y(Alignment::Center)
                )
                .on_press(SettingsMessage::OpenCurrentDataDir)
                .style(move |t, s| theme::secondary_button_style(&palette, t, s))
                .padding(10)
            ]
            .spacing(10)
            .align_y(Alignment::Center),
            row![
                button(theme::text_body(
                    localization.t("settings.move_to").to_string(),
                    ctx
                ))
                .on_press(SettingsMessage::PickMoveLocation)
                .style(move |t, s| theme::primary_button_style(&palette, t, s))
                .padding(10),
                Space::new().width(10),
                button(theme::text_body(
                    localization.t("settings.use_data_from").to_string(),
                    ctx
                ))
                .on_press(SettingsMessage::PickUseDataLocation)
                .style(move |t, s| theme::primary_button_style(&palette, t, s))
                .padding(10),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
            Space::new().width(10)
        ]
        .spacing(10);
        // --- 2. Channel & Version Pickers ---
        // Create "Channel Section" as a typed Element to resolve inference immediately
        let channel_section: Element<'a, SettingsMessage> = column![
            theme::text_small(localization.t("settings.update_channel"), ctx),
            theme::magic_pick_list_with_menu(
                pick_list(CHANNELS, Some(self.temp_settings.channel.as_str()), |c| {
                    SettingsMessage::ChannelChanged(c.to_string())
                },)
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
                .into(),
                ctx,
            )
        ]
        .spacing(5)
        .width(if is_compact {
            Length::Fill
        } else {
            Length::Shrink
        })
        .into();

        // Prepare version options: 0 (Latest) + available versions
        let version_options = {
            let mut opts = vec![VersionOption::Latest];
            opts.extend(
                self.available_versions
                    .iter()
                    .map(|&v| VersionOption::Specific(v as u32)),
            );
            opts
        };

        let selected_version = VersionOption::from(self.temp_settings.game_version);

        let selected_localized = if self.is_loading_versions {
            version_options.first().copied()
        } else {
            version_options
                .iter()
                .find(|&v| *v == selected_version)
                .copied()
        };

        // Create "Version Section" as a typed Element
        let version_section: Element<'a, SettingsMessage> = column![
            theme::text_small(localization.t("settings.target_version"), ctx),
            theme::magic_pick_list_with_menu(
                pick_list(version_options, selected_localized, |v| {
                    SettingsMessage::VersionSelected(u32::from(v))
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
                .into(),
                ctx,
            )
        ]
        .spacing(5)
        .width(if is_compact {
            Length::Fill
        } else {
            Length::Shrink
        })
        .into();

        let current_mode_ref = ONLINE_FIX_MODES
            .iter()
            .find(|&mode| *mode == self.temp_settings.online_fix_mode);
        let fix_mode_picker = row![
            theme::text_small(localization.t("settings.patch_mode"), ctx),
            theme::styled_dropdown(
                ONLINE_FIX_MODES,
                current_mode_ref,
                |mode: crate::config::OnlineFixMode| SettingsMessage::OnlineFixModeChanged(mode),
                localization.t("settings.patch_mode_placeholder"),
                ctx
            )
        ]
        .spacing(10)
        .align_y(Alignment::Center);

        let online_fix_checkbox = row![
            theme::magic_checkbox(
                checkbox(self.temp_settings.enable_online_fix)
                    .on_toggle(SettingsMessage::EnableOnlineFixToggled)
                    .style(move |t, s| theme::checkbox_style(&palette, t, s))
                    .into(),
                ctx
            ),
            column![
                theme::text_body(localization.t("settings.enable_online_fix"), ctx),
                theme::text_small(localization.t("settings.online_fix_desc"), ctx),
            ]
        ]
        .spacing(10)
        .align_y(Alignment::Center);

        let online_fix_section = column![
            online_fix_checkbox,
            if self.temp_settings.enable_online_fix {
                Into::<Element<'_, SettingsMessage>>::into(fix_mode_picker)
            } else {
                container(Space::new())
                    .style(move |t| theme::container_style_transparent(&palette, t))
                    .into()
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
            container(theme::text_small(
                localization.t("settings.storage.no_versions"),
                ctx,
            ))
            .width(Length::Fill)
            .padding(10)
            .style(move |t| theme::container_style_transparent(&palette, t))
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
                    container(theme::magic_row(
                        vec![
                            theme::text_small(label, ctx).into(),
                            Space::new().width(Length::Fill).into(), // Espacio flexible para empujar a la derecha
                            button(
                                container(theme::svg(
                                    svg(util::icons::icon(util::icons::FOLDER))
                                        .style(move |t, s| theme::svg_accent(&palette, t, s)),
                                    ctx,
                                ))
                                .center_x(Length::Fill)
                                .center_y(Length::Fill)
                                .style(move |t| theme::container_style_transparent(&palette, t)),
                            )
                            .on_press(SettingsMessage::OpenVersionFolder(version_val))
                            .width(40) // Ancho especifico para el boton
                            .style(move |t, s| theme::secondary_button_style(&palette, t, s))
                            .into(),
                            Space::new().width(5).into(),
                            button(
                                container(theme::svg(
                                    svg(util::icons::icon(util::icons::WRENCH))
                                        .style(move |t, s| theme::svg_accent(&palette, t, s)),
                                    ctx,
                                ))
                                .center_x(Length::Fill)
                                .center_y(Length::Fill)
                                .style(move |t| theme::container_style_transparent(&palette, t)),
                            )
                            .on_press(SettingsMessage::RepairVersion(version_val))
                            .width(40) // Ancho especifico para el boton
                            .style(move |t, s| theme::secondary_button_style(&palette, t, s))
                            .into(),
                            Space::new().width(5).into(),
                            button(
                                container(theme::svg(
                                    svg(util::icons::icon(util::icons::TRASH))
                                        .style(move |t, s| theme::svg_accent(&palette, t, s)),
                                    ctx,
                                ))
                                .center_x(Length::Fill)
                                .center_y(Length::Fill)
                                .style(move |t| theme::container_style_transparent(&palette, t)),
                            )
                            .on_press(SettingsMessage::DeleteVersion(version_val))
                            .width(40) // Ancho especifico para el boton
                            .style(move |t, s| theme::secondary_button_style(&palette, t, s))
                            .into(),
                        ],
                        ctx,
                    ))
                    .padding(5)
                    .style(move |t| theme::sidebar_style(&palette, t))
                    .width(Length::Fill)
                    .height(40),
                );
            }
            container(theme::magic_scrollable(
                scrollable(list)
                    .height(120)
                    .style(move |t, s| theme::scrollable_style(&palette, t, s))
                    .into(),
                ctx,
            ))
            .padding(iced::Padding {
                top: 0.0,
                right: 10.0,
                bottom: 0.0,
                left: 0.0,
            })
            .style(move |t| theme::container_style_transparent(&palette, t))
            .into()
        };

        let installed_manager = column![
            section_title(localization.t("settings.storage.title"), ctx),
            theme::text_small(localization.t("settings.storage.desc"), ctx),
            Space::new().height(5),
            installed_content
        ]
        .spacing(5);

        // --- CORRECCIoN 2b: Envolver retorno en tupla ---
        theme::page_container(theme::magic_column(
            vec![
                path_selector.into(),
                game_config.into(),
                installed_manager.into(),
            ],
            ctx,
        ))
        .into()
    }

    pub fn update(&mut self, message: SettingsMessage) -> Option<Message> {
        match message {
            SettingsMessage::TabSelected(tab) => {
                self.current_tab = tab.clone();
                // Iniciar carga asincrona cuando se selecciona la pestaÃ±a Java
                if matches!(tab, Tab::Java) && !self.java_info_loaded {
                    self.java_loading = true;
                    return Some(Message::Settings(SettingsMessage::JavaLoadingStarted));
                }
                None
            }
            SettingsMessage::EnableNewsToggled(val) => {
                self.temp_settings.enable_news = val;
                None
            }
            SettingsMessage::EnableOnlineFixToggled(val) => {
                self.temp_settings.enable_online_fix = val;
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
            SettingsMessage::PickUseDataLocation => {
                Some(Message::Settings(SettingsMessage::PickUseDataLocation))
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
            SettingsMessage::ScaleFactorChanged(val) => {
                self.temp_settings.scale_factor = val;
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
            SettingsMessage::JavaArgsAction(action) => {
                self.jvm_args_content.perform(action);
                // Sincronizar el contenido del editor con los settings para que se guarden
                self.temp_settings.java_args = self.jvm_args_content.text();
                None
            }
            SettingsMessage::JavaLoadingStarted => {
                // Este mensaje es manejado por el app.rs para iniciar la carga asincrona
                Some(Message::LoadJavaInfo)
            }
            SettingsMessage::JavaInfoLoaded => {
                self.java_loading = false;
                self.java_info_loaded = true;
                None
            }
            SettingsMessage::JavaVersionUpdated(version) => {
                self.java_version = Some(version);
                self.java_loading = false;
                self.java_info_loaded = true;
                None
            }
            SettingsMessage::DeleteVersion(v) => Some(Message::RequestDeleteVersion(v)),
            SettingsMessage::RepairVersion(v) => Some(Message::RequestRepairVersion(v)),
            SettingsMessage::OpenVersionFolder(v) => Some(Message::OpenVersionFolder(v)),
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
                Some(Message::Settings(SettingsMessage::LsdToggled(val)))
            }
            SettingsMessage::LsdHovered(val) => {
                Some(Message::Settings(SettingsMessage::LsdHovered(val)))
            }
            SettingsMessage::BaseThemeChanged(mode) => {
                self.temp_settings.theme.base_mode = mode;
                None
            }
            SettingsMessage::LanguageSelected(lang) => {
                Some(Message::LanguageChangedInSettings(lang.id))
            }
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
            SettingsMessage::ChannelChanged(val) => {
                self.temp_settings.channel = val.clone();
                self.is_loading_versions = true;
                self.available_versions.clear();
                Some(Message::RequestVersionCheck(val))
            }
            SettingsMessage::VersionSelected(version) => {
                self.temp_settings.game_version = version;
                None
            }
            SettingsMessage::OnlineFixModeChanged(mode) => {
                self.temp_settings.online_fix_mode = mode;
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
                localization.t("settings.tabs.launcher"),
                util::icons::SETTINGS,
                Tab::Launcher,
                &self.current_tab,
                is_compact,
                ctx,
            ),
            tab_button(
                localization.t("settings.tabs.game"),
                util::icons::GAMEPAD,
                Tab::Game,
                &self.current_tab,
                is_compact,
                ctx,
            ),
            tab_button(
                localization.t("settings.tabs.video"),
                util::icons::MONITOR,
                Tab::Video,
                &self.current_tab,
                is_compact,
                ctx,
            ),
            tab_button(
                localization.t("settings.tabs.java"),
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
            Tab::Video => theme::page_container(theme::magic_column(
                vec![
                    section_title(localization.t("settings.display"), ctx),
                    row![
                        theme::labeled_input(
                            localization.t("settings.width"),
                            &self.temp_settings.width.to_string(),
                            SettingsMessage::WidthChanged,
                            ctx,
                        ),
                        theme::labeled_input(
                            localization.t("settings.height"),
                            &self.temp_settings.height.to_string(),
                            SettingsMessage::HeightChanged,
                            ctx,
                        ),
                    ]
                    .spacing(20)
                    .into(),
                    column![
                        theme::text_small(
                            format!(
                                "{}: {:.2}x",
                                localization.t("settings.scale_factor"),
                                self.temp_settings.scale_factor
                            ),
                            ctx
                        ),
                        theme::magic_slider(
                            slider(
                                1.0..=2.0,
                                self.temp_settings.scale_factor,
                                SettingsMessage::ScaleFactorChanged,
                            )
                            .step(0.05)
                            .style(move |t, s| theme::slider_style(&palette, t, s))
                            .into(),
                            ctx
                        )
                    ]
                    .spacing(5)
                    .into(),
                    row![
                        theme::magic_checkbox(
                            checkbox(self.temp_settings.fullscreen)
                                .on_toggle(SettingsMessage::FullscreenToggled)
                                .style(move |t, s| theme::checkbox_style(&palette, t, s))
                                .into(),
                            ctx,
                        ),
                        theme::text_body(localization.t("settings.fullscreen"), ctx)
                    ]
                    .spacing(10)
                    .align_y(Alignment::Center)
                    .into(),
                ],
                ctx,
            ))
            .into(),
            Tab::Java => {
                let content = if self.java_loading {
                    theme::magic_column(
                        vec![theme::text_title(
                            localization.t("launcher.status.loading"),
                            ctx,
                        )],
                        ctx,
                    )
                } else {
                    theme::magic_column(
                        vec![
                            // Mostrar version de Java si esta disponible
                            if let Some(ref version) = self.java_version {
                                theme::text_title(
                                    &format!(
                                        "{}: {}",
                                        localization.t("settings.java_version"),
                                        version
                                    ),
                                    ctx,
                                )
                            } else {
                                theme::text_body(localization.t("settings.java_not_detected"), ctx)
                            },
                            section_title(localization.t("settings.java_memory"), ctx),
                            theme::text_body(
                                &format!(
                                    "{}: {} GB",
                                    localization.t("settings.min"),
                                    self.temp_settings.min_memory
                                ),
                                ctx,
                            ),
                            theme::magic_slider(
                                slider(
                                    1.0..=16.0,
                                    self.temp_settings.min_memory as f32,
                                    SettingsMessage::MinMemoryChanged,
                                )
                                .step(1.0)
                                .style(move |t, s| theme::slider_style(&palette, t, s))
                                .into(),
                                ctx,
                            ),
                            theme::text_body(
                                &format!(
                                    "{}: {} GB",
                                    localization.t("settings.max"),
                                    self.temp_settings.max_memory
                                ),
                                ctx,
                            ),
                            theme::magic_slider(
                                slider(
                                    1.0..=32.0,
                                    self.temp_settings.max_memory as f32,
                                    SettingsMessage::MaxMemoryChanged,
                                )
                                .step(1.0)
                                .style(move |t, s| theme::slider_style(&palette, t, s))
                                .into(),
                                ctx,
                            ),
                            section_title(localization.t("settings.jvm_args"), ctx),
                            theme::magic_text_area(
                                iced::widget::text_editor(&self.jvm_args_content)
                                    .on_action(|action| SettingsMessage::JavaArgsAction(action)) // Necesitas un mensaje nuevo
                                    .style(move |t, status| {
                                        theme::text_editor_style(&palette, t, status)
                                    })
                                    .into(),
                                ctx,
                            ),
                        ],
                        ctx,
                    )
                };

                theme::page_container(content).into()
            }
        };

        let footer = row![
            theme::magic_button(
                button(theme::text_small(localization.t("settings.cancel"), ctx))
                    .on_press(SettingsMessage::CloseModal)
                    .style(move |t, s| theme::secondary_button_style(&palette, t, s))
                    .padding(10) // Reduced visible scaling by keeping padding reasonable but text small
                    .into(),
                ctx,
            ),
            Space::new().width(10),
            theme::magic_button(
                button(theme::text_body(localization.t("settings.save"), ctx))
                    .on_press(SettingsMessage::SaveSettings)
                    .style(move |t, status| theme::primary_button_style(&palette, t, status))
                    .padding(10)
                    .into(),
                ctx,
            )
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

        let main_row = row![
            container(tabs)
                .padding(10)
                .style(move |t| theme::sidebar_style(&palette, t)),
            container(theme::magic_scrollable(
                scrollable(content)
                    .id(Id::new(SETTINGS_SCROLL_ID))
                    .style(move |t, status| theme::scrollable_style(&palette, t, status))
                    .into(),
                ctx,
            ))
            .padding(20)
            .width(Length::Fill)
            .style(move |t| theme::container_style_transparent(&palette, t)),
        ]
        .height(Length::Fill);

        let base_settings = theme::modal_shell(
            localization.t("settings.title").to_string(),
            main_row,
            Some(footer.into()),
            SettingsMessage::CloseModal,
            ctx,
        )
        .width(modal_width)
        .height(modal_height)
        .into();

        base_settings
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
            theme::text_body(label, ctx)
        ]
        .spacing(10)
        .align_y(Alignment::Center)
        .into()
    };

    theme::magic_button(
        button(
            container(content)
                .width(Length::Fill)
                .center_x(Length::Fill)
                .padding(10)
                .style(move |t| theme::container_style_transparent(&palette, t)),
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
        .into(),
        ctx,
    )
}

fn theme_mode_button<'a>(
    label: &str,
    mode: crate::config::BaseThemeMode,
    current: crate::config::BaseThemeMode,
    ctx: theme::UIContext,
) -> Element<'a, SettingsMessage> {
    let palette = ctx.palette;
    let is_active = mode == current;

    theme::magic_button(
        button(
            container(theme::text_small(label, ctx))
                .center_x(Length::Fill)
                .padding(8)
                .style(move |t| theme::container_style_transparent(&palette, t)),
        )
        .on_press(SettingsMessage::BaseThemeChanged(mode))
        .width(Length::Fill)
        .style(move |t, s| {
            if is_active {
                theme::active_tab_style(&palette, t, s)
            } else {
                theme::ghost_button_style(&palette, t, s)
            }
        })
        .into(),
        ctx,
    )
    .into()
}

fn section_title<'a>(
    label: impl Into<String>,
    ctx: theme::UIContext,
) -> Element<'a, SettingsMessage> {
    let label_str = label.into();
    theme::text_body(label_str, ctx)
}

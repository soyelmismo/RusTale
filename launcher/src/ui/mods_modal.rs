use crate::config::GameSettings;
use crate::game::curseforge::{CfMod, SearchResult};
use crate::game::mods::ModInfo;
use crate::game::zip_mods::PatchManifest;
use crate::{theme, util};
use iced::widget::{
    Space, button, column, container, image, row, scrollable, svg, text, text_input,
};
use iced::{Alignment, Color, ContentFit, Element, Length, Size, Task};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq)]
pub enum ModTab {
    Installed,
    Browse,
}

#[derive(Debug, Clone)]
pub enum ModsMessage {
    Close,
    SwitchTab(ModTab),

    // Local
    RefreshLocal,
    RefreshLocalBackground,
    ToggleLocal(ModInfo),
    DeleteLocal(ModInfo),
    OpenFolder,
    OpenPatchFolder,
    OpenJarFolder,
    ModsLoaded(Result<Vec<ModInfo>, String>),

    // Remote
    SearchChanged(String),
    SearchSubmit,
    NextPage,
    PrevPage,
    SearchLoaded(Result<SearchResult, String>),
    ImageLoaded(i32, Result<image::Handle, String>), // id del mod, resultado
    InstallMod(CfMod),
    ModInstalled(Result<String, String>, i32), // Nombre del archivo instalado, ID del mod
    OpenModPage(String),
    InstallZipPatch(std::path::PathBuf, GameSettings), // Ruta del archivo ZIP seleccionado
    ToggleZipPatch(String, bool),                      // ID, nuevo estado (true=activar)
    UninstallZipPatch(String, GameSettings),           // Mod ID
    PatchOperationFinished(Result<(), String>),
    ModsLoadedComplex(Result<(Vec<ModInfo>, Vec<crate::game::zip_mods::PatchManifest>), String>),
    OpenMods,
}

pub struct ModsState {
    pub is_open: bool,
    pub current_tab: ModTab,
    // Remote State
    pub search_query: String,
    pub remote_mods: Vec<CfMod>,
    pub current_page: u32,
    pub total_results: u32,
    pub page_size: u32,
    pub thumbnails: HashMap<i32, image::Handle>, // Cache de imágenes

    pub loading: bool,
    pub error: Option<String>,
    pub patch_mods: Vec<PatchManifest>,
    pub installed_mods: Vec<ModInfo>,
    pub temp_settings: GameSettings,

    pub installing_ids: HashSet<i32>,
    pub installed_ids: HashSet<i32>,
}

impl Default for ModsState {
    fn default() -> Self {
        Self {
            loading: false,
            error: None,
            is_open: false,
            current_tab: ModTab::Installed,
            search_query: String::new(),
            remote_mods: Vec::new(),
            current_page: 0,
            total_results: 0,
            page_size: 10,
            thumbnails: HashMap::new(),
            installed_mods: Vec::new(),
            patch_mods: Vec::new(),
            temp_settings: GameSettings::default(),
            installing_ids: HashSet::new(),
            installed_ids: HashSet::new(),
        }
    }
}

impl ModsState {
    pub fn new() -> Self {
        Self {
            is_open: false,
            current_tab: ModTab::Installed,
            search_query: String::new(),
            remote_mods: Vec::new(),
            current_page: 0,
            total_results: 0,
            page_size: 10,
            thumbnails: HashMap::new(),
            loading: false,
            error: None,
            patch_mods: Vec::new(),
            installed_mods: Vec::new(),
            temp_settings: GameSettings::default(),
            installing_ids: HashSet::new(),
            installed_ids: HashSet::new(),
        }
    }

    pub fn update(
        &mut self,
        message: ModsMessage,
        client: reqwest::Client,
        base_dir: std::path::PathBuf,
        settings: GameSettings,
    ) -> Task<ModsMessage> {
        match message {
            ModsMessage::Close => {
                self.is_open = false;
                Task::none()
            }
            ModsMessage::RefreshLocal | ModsMessage::OpenMods => {
                self.is_open = true;
                self.loading = true;

                // Limpiamos visualmente para dar feedback de carga
                self.installed_mods.clear();
                self.patch_mods.clear();
                self.installing_ids.clear();
                self.installed_ids.clear();

                self.load_mods_task(base_dir, settings)
            }

            ModsMessage::RefreshLocalBackground => self.load_mods_task(base_dir, settings),

            ModsMessage::ModsLoaded(res) => {
                self.loading = false;
                match res {
                    Ok(mods) => self.installed_mods = mods,
                    Err(e) => self.error = Some(e),
                }
                Task::none()
            }

            ModsMessage::ModsLoadedComplex(res) => {
                if self.loading {
                    self.loading = false;
                }
                match res {
                    Ok((jars, patches)) => {
                        self.installed_mods = jars;
                        self.patch_mods = patches;
                        self.error = None;
                    }
                    Err(e) => {
                        if self.is_open {
                            self.error = Some(format!("Error loading mods: {}", e));
                        }
                    }
                }
                Task::none()
            }

            ModsMessage::ToggleLocal(mod_info) => {
                self.loading = true;
                let base_dir = base_dir.clone();
                let channel = settings.channel.clone();
                let ver = if settings.game_version == 0 {
                    "latest".to_string()
                } else {
                    settings.game_version.to_string()
                };

                Task::perform(
                    async move {
                        crate::game::mods::toggle_mod(&base_dir, &channel, &ver, &mod_info)
                            .await
                            .unwrap();
                        Ok::<(), String>(())
                    },
                    |_| ModsMessage::RefreshLocalBackground,
                )
            }

            ModsMessage::DeleteLocal(mod_info) => {
                self.loading = true;
                Task::perform(
                    async move {
                        let _ = crate::game::mods::delete_mod(&mod_info).await;
                        Ok::<(), String>(())
                    },
                    |_| ModsMessage::RefreshLocalBackground,
                )
            }

            ModsMessage::OpenFolder => {
                let paths = crate::game::GamePaths::new(base_dir);
                let ver = if settings.game_version == 0 {
                    "latest".to_string()
                } else {
                    settings.game_version.to_string()
                };
                let mods_path = paths.mods_dir(&settings.channel, &ver);
                crate::util::open_path(mods_path);
                Task::none()
            }

            ModsMessage::OpenPatchFolder => {
                let paths = crate::game::GamePaths::new(base_dir);
                let ver = if settings.game_version == 0 {
                    "latest".to_string()
                } else {
                    settings.game_version.to_string()
                };
                let dir = paths.core_patches_dir(&settings.channel, &ver);
                crate::util::open_path(dir);
                Task::none()
            }

            ModsMessage::OpenJarFolder => {
                let paths = crate::game::GamePaths::new(base_dir);
                let ver = if settings.game_version == 0 {
                    "latest".to_string()
                } else {
                    settings.game_version.to_string()
                };
                let dir = paths.mods_dir(&settings.channel, &ver);
                crate::util::open_path(dir);
                Task::none()
            }

            ModsMessage::SwitchTab(tab) => {
                self.current_tab = tab;
                if self.current_tab == ModTab::Browse && self.remote_mods.is_empty() {
                    return self.perform_search(client);
                }
                Task::none()
            }
            ModsMessage::SearchChanged(query) => {
                self.search_query = query;
                Task::none()
            }
            ModsMessage::SearchSubmit => {
                self.current_page = 0;
                self.perform_search(client)
            }

            ModsMessage::NextPage => {
                if (self.current_page + 1) * self.page_size < self.total_results {
                    self.current_page += 1;
                    return self.perform_search(client);
                }
                Task::none()
            }
            ModsMessage::PrevPage => {
                if self.current_page > 0 {
                    self.current_page -= 1;
                    return self.perform_search(client);
                }
                Task::none()
            }
            ModsMessage::SearchLoaded(res) => {
                self.loading = false;
                match res {
                    Ok(result) => {
                        self.remote_mods = result.mods;
                        self.total_results = result.total_count;
                        self.error = None;

                        // Cargar imágenes
                        let mut tasks = Vec::new();
                        for mod_item in &self.remote_mods {
                            if let Some(logo) = &mod_item.logo {
                                if !self.thumbnails.contains_key(&mod_item.id) {
                                    let url = logo.thumbnail_url.clone();
                                    let id = mod_item.id;
                                    let c = client.clone();
                                    tasks.push(Task::perform(
                                        async move {
                                            let res =
                                                crate::util::image_cache::load_image(&c, &url)
                                                    .await;
                                            (id, res.map_err(|e| e.to_string()))
                                        },
                                        |(id, res)| ModsMessage::ImageLoaded(id, res),
                                    ));
                                }
                            }
                        }
                        if tasks.is_empty() {
                            Task::none()
                        } else {
                            Task::batch(tasks)
                        }
                    }
                    Err(e) => {
                        self.error = Some(e);
                        Task::none()
                    }
                }
            }
            ModsMessage::ImageLoaded(id, res) => {
                if let Ok(handle) = res {
                    self.thumbnails.insert(id, handle);
                }
                Task::none()
            }
            ModsMessage::InstallMod(cf_mod) => {
                self.installing_ids.insert(cf_mod.id);
                let mod_id = cf_mod.id;

                if let Some(file) = cf_mod.latest_files.first() {
                    if let Some(url) = &file.download_url {
                        let url_clone = url.clone();
                        let file_name = file.file_name.clone();
                        let base_dir_clone = base_dir.clone();

                        return Task::perform(
                            async move {
                                let channel = settings.channel.clone();
                                let ver = if settings.game_version == 0 {
                                    "latest".to_string()
                                } else {
                                    settings.game_version.to_string()
                                };

                                let (mods_dir, _) = crate::game::mods::ensure_mod_dirs(
                                    &base_dir_clone,
                                    &channel,
                                    &ver,
                                )
                                .await;
                                let dest = mods_dir.join(&file_name);

                                // 1. Descargar
                                match crate::game::downloader::download_file(
                                    &client,
                                    &url_clone,
                                    &dest,
                                    |_, _| {},
                                    None,
                                )
                                .await
                                {
                                    Ok(_) => {
                                        // 2. VALIDACIÓN: Chequear tamaño del archivo
                                        if let Ok(metadata) = tokio::fs::metadata(&dest).await {
                                            if metadata.len() == 0 {
                                                let _ = tokio::fs::remove_file(&dest).await;
                                                return Err("Downloaded file is empty (0 bytes)"
                                                    .to_string());
                                            }
                                        }
                                        Ok(file_name)
                                    }
                                    Err(e) => Err(e.to_string()),
                                }
                            },
                            move |res| ModsMessage::ModInstalled(res, mod_id),
                        );
                    }
                }
                self.installing_ids.remove(&mod_id);
                self.error = Some("No download URL available".to_string());
                Task::none()
            }
            ModsMessage::OpenModPage(url) => {
                let _ = open::that(url);
                Task::none()
            }

            ModsMessage::ModInstalled(res, mod_id) => {
                self.installing_ids.remove(&mod_id);
                match res {
                    Ok(file_name) => {
                        self.installed_ids.insert(mod_id);
                        let base_dir_clone = base_dir.clone();
                        let channel = settings.channel.clone();
                        let ver = if settings.game_version == 0 {
                            "latest".to_string()
                        } else {
                            settings.game_version.to_string()
                        };

                        // Comprobar si es un parche
                        let (mods_dir, _) = tokio::task::block_in_place(|| {
                            futures::executor::block_on(crate::game::mods::ensure_mod_dirs(
                                &base_dir_clone,
                                &channel,
                                &ver,
                            ))
                        });
                        let full_path = mods_dir.join(&file_name);

                        if file_name.ends_with(".zip")
                            && crate::game::zip_mods::is_patch_mod(&full_path)
                        {
                            Task::done(ModsMessage::InstallZipPatch(full_path, settings))
                        } else {
                            Task::done(ModsMessage::RefreshLocalBackground)
                        }
                    }
                    Err(e) => {
                        self.error = Some(format!("Install failed: {}", e));
                        Task::none()
                    }
                }
            }

            ModsMessage::InstallZipPatch(zip_path, settings) => {
                self.loading = true;
                let base_dir = base_dir.clone();

                Task::perform(
                    async move {
                        let paths = crate::game::GamePaths::new(base_dir);
                        let channel = settings.channel.clone();
                        let ver = if settings.game_version == 0 {
                            "latest".to_string()
                        } else {
                            settings.game_version.to_string()
                        };

                        let game_dir = paths.version_dir(&channel, &ver);
                        let patches_dir = paths.core_patches_dir(&channel, &ver);
                        let mod_name = zip_path
                            .file_stem()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string();
                        let zip_path_to_clean = zip_path.clone();

                        tokio::task::spawn_blocking(move || {
                            crate::game::zip_mods::install_new_patch(
                                zip_path,
                                game_dir,
                                patches_dir,
                                mod_name,
                            )
                        })
                        .await
                        .map_err(|e| e.to_string())?
                        .map_err(|e| e.to_string())?;

                        let _ = tokio::fs::remove_file(zip_path_to_clean).await;
                        Ok(())
                    },
                    ModsMessage::PatchOperationFinished,
                )
            }

            ModsMessage::ToggleZipPatch(mod_id, enable) => {
                self.loading = true;
                let base_dir = base_dir.clone();
                let settings = settings.clone();

                Task::perform(
                    async move {
                        let paths = crate::game::GamePaths::new(base_dir);
                        let channel = settings.channel;
                        let ver = if settings.game_version == 0 {
                            "latest".to_string()
                        } else {
                            settings.game_version.to_string()
                        };

                        let game_dir = paths.version_dir(&channel, &ver);
                        let patches_dir = paths.core_patches_dir(&channel, &ver);

                        tokio::task::spawn_blocking(move || {
                            if enable {
                                crate::game::zip_mods::enable_patch(game_dir, patches_dir, &mod_id)
                            } else {
                                crate::game::zip_mods::disable_patch(game_dir, patches_dir, &mod_id)
                            }
                        })
                        .await
                        .unwrap()
                        .map_err(|e| e.to_string())
                    },
                    ModsMessage::PatchOperationFinished,
                )
            }

            ModsMessage::UninstallZipPatch(mod_id, settings) => {
                self.loading = true;
                let base_dir = base_dir.clone();

                Task::perform(
                    async move {
                        let paths = crate::game::GamePaths::new(base_dir);
                        let channel = settings.channel.clone();
                        let ver = if settings.game_version == 0 {
                            "latest".to_string()
                        } else {
                            settings.game_version.to_string()
                        };

                        let game_dir = paths.version_dir(&channel, &ver);
                        let patches_dir = paths.core_patches_dir(&channel, &ver);

                        tokio::task::spawn_blocking(move || {
                            crate::game::zip_mods::uninstall_patch(game_dir, patches_dir, &mod_id)
                        })
                        .await
                        .map_err(|e| e.to_string())?
                        .map_err(|e| e.to_string())
                    },
                    ModsMessage::PatchOperationFinished,
                )
            }

            ModsMessage::PatchOperationFinished(res) => {
                self.loading = false;
                match res {
                    Ok(_) => Task::done(ModsMessage::RefreshLocalBackground),
                    Err(e) => {
                        self.error = Some(format!("Patch operation failed: {}", e));
                        Task::none()
                    }
                }
            }
        }
    }

    fn load_mods_task(
        &self,
        base_dir: std::path::PathBuf,
        settings: GameSettings,
    ) -> Task<ModsMessage> {
        let base_dir_clone = base_dir.clone();
        let channel = settings.channel.clone();
        let version_str = if settings.game_version == 0 {
            "latest".to_string()
        } else {
            settings.game_version.to_string()
        };

        Task::perform(
            async move {
                let jars = crate::game::mods::list_mods(&base_dir_clone, &channel, &version_str)
                    .await
                    .map_err(|e| e.to_string())?;

                let paths = crate::game::GamePaths::new(base_dir_clone);
                let patches_dir = paths.core_patches_dir(&channel, &version_str);
                let patches =
                    crate::game::zip_mods::list_patches(patches_dir).map_err(|e| e.to_string())?;

                Ok((jars, patches))
            },
            |res| ModsMessage::ModsLoadedComplex(res),
        )
    }

    fn perform_search(&mut self, client: reqwest::Client) -> Task<ModsMessage> {
        self.loading = true;
        let query = self.search_query.clone();
        let idx = self.current_page * self.page_size;
        let limit = self.page_size;

        Task::perform(
            async move {
                crate::game::curseforge::search_mods(&client, &query, idx, limit)
                    .await
                    .map_err(|e| e.to_string())
            },
            ModsMessage::SearchLoaded,
        )
    }

    pub fn view<'a>(
        &'a self,
        loc: &'a crate::lang::Localization,
        window_size: Size,
    ) -> Element<'a, ModsMessage> {
        let is_compact = window_size.width < 600.0;

        // --- Header ---
        let title = text("MOD MANAGER")
            .size(if is_compact { 16 } else { 20 })
            .font(iced::font::Font::MONOSPACE);
        let close_btn = button(text(loc.t("common.close").to_string()).size(12))
            .on_press(ModsMessage::Close)
            .style(theme::secondary_button_style)
            .padding(if is_compact { 5 } else { 10 });

        let header = row![title, Space::new().width(Length::Fill), close_btn]
            .align_y(Alignment::Center)
            .padding(if is_compact { 10 } else { 20 });

        // --- Tabs ---
        let tabs = row![
            tab_btn(
                "INSTALLED",
                self.current_tab == ModTab::Installed,
                is_compact
            )
            .on_press(ModsMessage::SwitchTab(ModTab::Installed)),
            tab_btn("BROWSE", self.current_tab == ModTab::Browse, is_compact)
                .on_press(ModsMessage::SwitchTab(ModTab::Browse)),
        ]
        .spacing(10)
        .padding(if is_compact { 10 } else { 20 });

        // --- Content ---
        let content = match self.current_tab {
            ModTab::Installed => self.view_installed(loc),
            ModTab::Browse => self.view_browse(loc, is_compact),
        };

        // Dimensiones dinámicas
        let modal_width = if window_size.width < 850.0 {
            Length::Fill
        } else {
            Length::Fixed(800.0)
        };
        let modal_height = if window_size.height < 650.0 {
            Length::Fill
        } else {
            Length::Fixed(600.0)
        };
        let outer_padding = if is_compact { 5 } else { 0 };

        container(column![
            header,
            tabs,
            container(content)
                .padding(if is_compact { 10 } else { 20 })
                .height(Length::Fill),
        ])
        .width(modal_width)
        .height(modal_height)
        .padding(outer_padding)
        .style(theme::modal_container)
        .into()
    }

    fn view_installed<'a>(
        &'a self,
        _loc: &'a crate::lang::Localization,
    ) -> Element<'a, ModsMessage> {
        // Si no hay nada en absoluto
        if self.installed_mods.is_empty() && self.patch_mods.is_empty() {
            return container(
                text("No mods installed")
                    .size(16)
                    .color(Color::from_rgb(0.7, 0.7, 0.7)),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into();
        }

        let mut content = column![].spacing(20);

        // 1. SECCIÓN PARCHES (.zip) - Mostrar primero si existen
        if !self.patch_mods.is_empty() {
            let header = row![
                text("CORE PATCHES (ZIP)")
                    .size(14)
                    .color(theme::ACCENT_ORANGE)
                    .font(iced::font::Font::MONOSPACE),
                Space::new().width(Length::Fill),
                button(
                    row![
                        svg(util::icons::icon(util::icons::FOLDER))
                            .width(14)
                            .height(14)
                            .style(theme::svg_accent),
                        text("Open Folder").size(10)
                    ]
                    .spacing(5)
                    .align_y(Alignment::Center)
                )
                .on_press(ModsMessage::OpenPatchFolder)
                .style(theme::secondary_button_style)
                .padding(5)
            ]
            .align_y(Alignment::Center);

            let mut patch_list = column![header].spacing(10);

            for patch in &self.patch_mods {
                patch_list = patch_list.push(patch_row(patch, &self.temp_settings));
            }
            content = content.push(patch_list);
        }

        // 2. SECCIÓN MODS (.jar)
        let header_jar = row![
            text("MODS (JAR)")
                .size(14)
                .color(theme::ACCENT_ORANGE)
                .font(iced::font::Font::MONOSPACE),
            Space::new().width(Length::Fill),
            button(
                row![
                    svg(util::icons::icon(util::icons::FOLDER))
                        .width(14)
                        .height(14)
                        .style(theme::svg_accent),
                    text("Open Folder").size(10)
                ]
                .spacing(5)
                .align_y(Alignment::Center)
            )
            .on_press(ModsMessage::OpenJarFolder)
            .style(theme::secondary_button_style)
            .padding(5)
        ]
        .align_y(Alignment::Center);

        let mut mod_list = column![header_jar].spacing(10);

        if !self.installed_mods.is_empty() {
            for m in &self.installed_mods {
                mod_list = mod_list.push(mod_row(m));
            }
        } else if self.patch_mods.is_empty() {
            // Solo si no hay parches tampoco mostramos un texto placeholder
            mod_list = mod_list.push(
                text("No JAR mods installed")
                    .size(12)
                    .color(Color::from_rgb(0.5, 0.5, 0.5)),
            );
        }

        content = content.push(mod_list);

        container(
            scrollable(content)
                .height(Length::Fill)
                .style(theme::scrollable_style),
        )
        .padding(iced::Padding {
            top: 0.0,
            right: 0.0,
            bottom: 0.0,
            left: 0.0,
        })
        .into()
    }

    fn view_browse<'a>(
        &'a self,
        _loc: &'a crate::lang::Localization,
        is_compact: bool,
    ) -> Element<'a, ModsMessage> {
        // Barra de búsqueda
        let search_input = text_input("Search mods...", &self.search_query)
            .on_input(ModsMessage::SearchChanged)
            .on_submit(ModsMessage::SearchSubmit)
            .padding(10)
            .style(theme::text_input_style)
            .width(Length::Fill);

        let search_btn = button(text("Search").size(14))
            .on_press(ModsMessage::SearchSubmit)
            .style(theme::primary_button_style)
            .padding(10);

        let search_bar = row![search_input, search_btn].spacing(10);

        let content: Element<'a, ModsMessage> = if self.loading {
            container(text("Loading...").size(20))
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .height(Length::Fill)
                .into()
        } else if let Some(err) = &self.error {
            container(text(err).color(Color::from_rgb(1.0, 0.4, 0.4)))
                .center_x(Length::Fill)
                .into()
        } else {
            let list = column(
                self.remote_mods
                    .iter()
                    .map(|m| self.view_remote_card(m))
                    .collect::<Vec<_>>(),
            )
            .spacing(10);

            container(
                scrollable(list)
                    .height(Length::Fill)
                    .style(theme::scrollable_style),
            )
            .padding(iced::Padding {
                top: 0.0,
                right: 0.0, // Small padding for the scrollbar itself, but not the 25.0 from before
                bottom: 0.0,
                left: 0.0,
            })
            .into()
        };

        let prev_btn = button(text("< Prev").size(if is_compact { 12 } else { 14 }))
            .style(theme::secondary_button_style)
            .padding(if is_compact { 6 } else { 10 });

        let prev_btn = if self.current_page > 0 {
            prev_btn.on_press(ModsMessage::PrevPage)
        } else {
            prev_btn // Sin on_press = disabled
        };

        // Paginación
        let pagination = row![
            prev_btn,
            text(format!("Page {}", self.current_page + 1)).size(if is_compact { 12 } else { 14 }),
            button(text("Next >").size(if is_compact { 12 } else { 14 }))
                .on_press(ModsMessage::NextPage)
                .style(theme::secondary_button_style)
                .padding(if is_compact { 6 } else { 10 }),
        ]
        .spacing(if is_compact { 10 } else { 20 })
        .align_y(Alignment::Center)
        .width(Length::Fill);

        column![search_bar, content, pagination].spacing(15).into()
    }

    fn view_remote_card<'a>(
        &'a self,
        cf_mod: &'a crate::game::curseforge::CfMod,
    ) -> Element<'a, ModsMessage> {
        let is_installing = self.installing_ids.contains(&cf_mod.id);
        let is_installed = self.installed_ids.contains(&cf_mod.id);

        let action_btn = if is_installing {
            button(text("Downloading...").size(12)).style(theme::ghost_button_style)
        } else if is_installed {
            button(text("Installed").size(12)).style(theme::success_button_style)
        } else {
            button(text("Install").size(12))
                .on_press(ModsMessage::InstallMod(cf_mod.clone()))
                .style(theme::primary_button_style)
        }
        .padding(8);

        let thumb: Element<'a, ModsMessage> = if let Some(handle) = self.thumbnails.get(&cf_mod.id)
        {
            image(handle.clone())
                .width(50)
                .height(50)
                .content_fit(ContentFit::Cover)
                .into()
        } else {
            container(Space::new())
                .width(50)
                .height(50)
                .style(|_t: &iced::Theme| container::Style {
                    background: Some(iced::Background::Color(Color::from_rgb(0.2, 0.2, 0.2))),
                    ..Default::default()
                })
                .into()
        };

        container(
            row![
                thumb,
                column![
                    text(&cf_mod.name).size(16).color(Color::WHITE),
                    text(&cf_mod.summary)
                        .size(12)
                        .color(Color::from_rgb(0.7, 0.7, 0.7))
                        .width(Length::Fill),
                    text(format!("Downloads: {:.0}", cf_mod.download_count))
                        .size(10)
                        .color(theme::ACCENT_ORANGE),
                ]
                .spacing(4)
                .width(Length::Fill),
                action_btn
            ]
            .spacing(15)
            .align_y(Alignment::Center),
        )
        .padding(iced::Padding {
            top: 10.0,
            right: 20.0, // Extra separation for the button
            bottom: 10.0,
            left: 10.0,
        })
        .style(theme::card_style)
        .into()
    }
}

pub fn mod_row<'a>(mod_info: &'a ModInfo) -> Element<'a, ModsMessage> {
    let status_color = if mod_info.enabled {
        theme::ACCENT_GREEN
    } else {
        Color::from_rgb(0.5, 0.5, 0.5)
    };

    let toggle_btn = if mod_info.enabled {
        button(text("DISABLE")).style(theme::secondary_button_style)
    } else {
        button(text("ENABLE")).style(theme::primary_button_style)
    }
    .on_press(ModsMessage::ToggleLocal(mod_info.clone()));

    use iced::widget::svg;
    let delete_btn = button(
        svg(util::icons::icon(util::icons::TRASH))
            .width(14)
            .height(14)
            .style(|_t, _s| iced::widget::svg::Style {
                color: Some(Color::BLACK),
            }),
    )
    .on_press(ModsMessage::DeleteLocal(mod_info.clone()))
    .style(theme::danger_button_style)
    .padding(8);

    container(
        row![
            text(&mod_info.name).size(14).width(Length::Fill),
            text(if mod_info.enabled {
                "ACTIVE"
            } else {
                "DISABLED"
            })
            .size(12)
            .color(status_color),
            toggle_btn,
            delete_btn
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    )
    .padding(10)
    .style(theme::card_style)
    .into()
}

// Helper para botones de tab
fn tab_btn<'a>(label: &'a str, active: bool, compact: bool) -> button::Button<'a, ModsMessage> {
    button(
        text(label)
            .size(if compact { 10 } else { 12 })
            .align_x(iced::alignment::Horizontal::Center),
    )
    .width(Length::Fill)
    .padding(if compact { 6 } else { 8 })
    .style(if active {
        theme::active_tab_style
    } else {
        theme::ghost_button_style
    })
}
fn patch_row<'a>(patch: &'a PatchManifest, _curr: &GameSettings) -> Element<'a, ModsMessage> {
    let status_color = if patch.enabled {
        theme::ACCENT_GREEN
    } else {
        Color::from_rgb(0.5, 0.5, 0.5)
    };

    let toggle_btn = if patch.enabled {
        button(text("DISABLE"))
            .style(theme::secondary_button_style)
            .on_press(ModsMessage::ToggleZipPatch(patch.mod_id.clone(), false))
    } else {
        button(text("ENABLE"))
            .style(theme::primary_button_style)
            .on_press(ModsMessage::ToggleZipPatch(patch.mod_id.clone(), true))
    };

    let delete_btn = button(
        svg(util::icons::icon(util::icons::TRASH))
            .width(14)
            .height(14)
            .style(|_t, _s| iced::widget::svg::Style {
                color: Some(Color::BLACK),
            }),
    )
    .on_press(ModsMessage::UninstallZipPatch(
        patch.mod_id.clone(),
        _curr.clone(),
    ))
    .style(theme::danger_button_style)
    .padding(5);

    container(
        row![
            text("📦").size(20),
            column![
                text(&patch.mod_name).size(14),
                row![
                    text(if patch.enabled { "ACTIVE" } else { "DISABLED" })
                        .size(10)
                        .color(status_color),
                    text("|").size(10).color(Color::from_rgb(0.3, 0.3, 0.3)),
                    text(patch.install_date.format("%Y-%m-%d").to_string())
                        .size(10)
                        .color(Color::from_rgb(0.5, 0.5, 0.5))
                ]
                .spacing(5)
            ]
            .width(Length::Fill),
            toggle_btn,
            delete_btn
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    )
    .padding(10)
    .style(theme::card_style)
    .into()
}

use crate::config::GameSettings;
use crate::game::mods::{InstalledModMetadata, ModInfo, ModInstallationRequest};
use crate::game::mods_api::{GenericMod, SearchResults};
use crate::game::zip_mods::PatchManifest;
use crate::{theme, util};
use iced::widget::{
    Id, Space, button, column, container, image, pick_list, row, scrollable, svg, text_input,
};
use iced::{Alignment, Color, ContentFit, Element, Length, Renderer, Size, Task, Theme};
use std::collections::{HashMap, HashSet};


const MODS_SCROLL_ID: &str = "mods_scroll_area";

#[derive(Debug, Clone, PartialEq)]
pub enum ModTab {
    Installed,
    Browse,
}

#[derive(Debug, Clone)]
pub enum ModsMessage {
    Close,
    SwitchTab(ModTab),
    RefreshLocal,
    RefreshLocalBackground,
    ToggleLocal(ModInfo),
    DeleteLocal(ModInfo),
    OpenFolder,
    OpenPatchFolder,
    OpenJarFolder,
    ModsLoaded(Result<Vec<ModInfo>, String>),
    SearchChanged(String),
    SearchSubmit,
    NextPage,
    PrevPage,
    SearchLoaded(Result<SearchResults, String>),
    ImageLoaded(String, Result<image::Handle, String>),
    InstallMod(GenericMod, Option<String>),
    ModInstalled(Result<String, String>, String), // Devuelve ID string
    OpenModPage(String),
    InstallZipPatch(
        std::path::PathBuf,
        Option<InstalledModMetadata>,
    ),
    ToggleZipPatch(String, bool),
    UninstallZipPatch(String, GameSettings),
    PatchOperationFinished(Result<(), String>),
    ModsLoadedComplex(Result<(Vec<ModInfo>, Vec<crate::game::zip_mods::PatchManifest>), String>),

    // --- VIEWPORT OPTIMIZATION MESSAGES ---
    ScrollOffsetChanged(f32),
    UpdateViewportHeight(f32),
    OpenMods,
    CheckForUpdates,
    UpdateMod(ModInfo),
    UpdateModToVersion(ModInfo, String, bool), // (ModInfo, FileID, is_patch)
    UpdateModStart(ModInfo),
    UpdateModDownloaded(Result<(String, ModInfo, String, bool), String>),
    VersionSelected {
        mod_id: String,
        file_id: String,
    },
    LoadVersions(String), // mod_id
    ToCore(crate::core::signals::ToCore),
}

#[derive(Debug, Clone)]
pub struct ModsState {
    pub is_open: bool,
    pub current_tab: ModTab,
    pub search_query: String,
    pub remote_mods: Vec<GenericMod>,
    pub current_page: u32,
    pub total_results: u32,
    pub page_size: u32,
    // Removed local thumbnails cache, using UiResources
    pub loading: bool,
    pub error: Option<String>,
    pub patch_mods: Vec<PatchManifest>,
    pub installed_mods: Vec<crate::game::mods::ModInfo>,
    pub temp_settings: GameSettings,
    pub installing_ids: HashSet<String>,
    pub installed_ids: HashSet<String>,
    pub installing_mods: HashMap<String, GenericMod>,
    pub checking_updates: bool,
    pub mods_with_updates: HashSet<String>, // Ahora contiene remote_ids
    // Cache para evitar recalculos en view()
    pub update_status_cache: HashMap<String, bool>, // file_name/mod_id -> has_update
    // Mapa para recordar que version selecciono el usuario en la UI para cada mod (Browse tab)
    pub selected_versions: HashMap<String, String>,
    // Set de mods que estan cargando versiones actualmente
    pub loading_versions: HashSet<String>,
    // Cache de versiones cargadas bajo demanda para cada mod
    pub cached_versions: HashMap<String, Vec<crate::game::mods_api::GenericFile>>,

    // --- VIEWPORT OPTIMIZATION FOR MODS LISTS ---
    pub scroll_offset: f32,   // Posición actual del scroll
    pub viewport_height: f32, // Altura del área visible
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
            // Removed thumbnails initialization
            installed_mods: Vec::new(),
            patch_mods: Vec::new(),
            temp_settings: GameSettings::default(),
            installing_ids: HashSet::new(),
            installed_ids: HashSet::new(),
            installing_mods: HashMap::new(),
            checking_updates: false,
            mods_with_updates: HashSet::new(),
            update_status_cache: HashMap::new(),
            selected_versions: HashMap::new(),
            loading_versions: HashSet::new(),
            cached_versions: HashMap::new(),

            // --- VIEWPORT OPTIMIZATION ---
            scroll_offset: 0.0,
            viewport_height: 400.0, // Altura inicial estimada
        }
    }
}

impl ModsState {
    /// Helper: Obtiene string de versión sin clonar innecesariamente
    fn version_str(version: u32) -> String {
        if version == 0 {
            "latest".to_string()
        } else {
            version.to_string()
        }
    }

    /// --- VIEWPORT OPTIMIZATION HELPERS ---
    /// Calcula qué elementos son visibles basados en scroll y viewport
    fn get_visible_range(
        &self,
        item_height: f32,
        total_items: usize,
        buffer: usize,
    ) -> (usize, usize) {
        if self.viewport_height <= 0.0 || total_items == 0 {
            return (0, total_items.min(buffer * 2));
        }

        // Calcular índice de inicio basado en scroll offset
        let start_index = (self.scroll_offset / item_height).floor() as usize;
        let start_index = start_index.saturating_sub(buffer); // Buffer arriba

        // Calcular cuántos elementos caben en el viewport
        let visible_count = (self.viewport_height / item_height).ceil() as usize;
        let end_index = (start_index + visible_count + buffer).min(total_items); // Buffer abajo

        (start_index, end_index)
    }

    pub fn new() -> Self {
        Self::default()
    }

    // Nuevo método para limpieza agresiva de memoria
    pub fn clear_heavy_data(&mut self) {
        // removed thumbnails.clear() - managed by orchestra/resources
        self.remote_mods.clear(); // Liberar lista de búsqueda
        self.current_page = 0;
        self.installed_mods.clear(); // Opcional, o mantener caché
        self.patch_mods.clear();
        self.update_status_cache.clear();
        self.selected_versions.clear();
        self.loading_versions.clear();
        self.cached_versions.clear(); // CRITICAL: Clear heavy version cache
        self.installing_ids.clear();
        self.installed_ids.clear();
        self.installing_mods.clear();
        self.mods_with_updates.clear();
    }

    /// CHANGE: Make this method public and more aggressive - called when modal closes
    pub fn reset_state(&mut self) {
        // Nuke heavy vectors
        self.remote_mods = Vec::new(); 
        self.cached_versions.clear();
        self.installing_mods.clear();
        // Keep installed_mods as they are cached from local disk (fast enough)
        // or clear them too if you want extreme low RAM
        // self.installed_mods.clear(); 
        
        self.search_query.clear();
        self.current_page = 0;
        self.is_open = false;
        
        // Additional cleanup for consistency
        self.loading = false;
        self.error = None;
        self.installing_ids.clear();
        self.installed_ids.clear();
        self.mods_with_updates.clear();
        self.update_status_cache.clear();
        self.selected_versions.clear();
        self.loading_versions.clear();
        self.patch_mods.clear();
    }

    /// Close the modal and release ALL memory-heavy vectors immediately.
    /// Call this whenever the modal is dismissed.
    pub fn close(&mut self) {
        self.is_open = false;
        // Release ALL heavy vectors immediately
        self.installed_mods = Vec::new();
        self.remote_mods = Vec::new();
        self.patch_mods = Vec::new();
        self.cached_versions.clear();
        self.installing_mods.clear();
        self.installing_ids.clear();
        self.installed_ids.clear();
        self.mods_with_updates.clear();
        self.update_status_cache.clear();
        self.selected_versions.clear();
        self.loading_versions.clear();
        
        // Force shrink to free heap
        self.installed_mods.shrink_to_fit();
        self.remote_mods.shrink_to_fit();
        self.patch_mods.shrink_to_fit();
        self.cached_versions.shrink_to_fit();
        self.installing_mods.shrink_to_fit();
        self.update_status_cache.shrink_to_fit();
        self.selected_versions.shrink_to_fit();

        // Keep only essential config
        self.search_query.clear();
        self.current_page = 0;
        self.loading = false;
        self.error = None;
    }

    /// Force trim the LRU cache to a specific size
    pub fn trim_thumbnails(&mut self, _size: usize) {
        // NO-OP: Managed globally by UiResources
    }

    pub fn update(
        &mut self,
        message: ModsMessage,
        _client: rustale_shared::reqwest::Client,
        base_dir: std::path::PathBuf,
        settings: GameSettings,
        resources: &mut crate::ui::resources::UiResources
    ) -> Task<ModsMessage> {
        match message {
            ModsMessage::Close => {
                self.close(); // Aggressively release all heavy vectors
                Task::none()
            }
            ModsMessage::RefreshLocal | ModsMessage::OpenMods => {
                self.is_open = true;
                self.loading = true;
                self.installed_mods.clear();
                self.patch_mods.clear();
                self.installing_ids.clear();
                self.installed_ids.clear();
                self.installing_mods.clear();
                self.mods_with_updates.clear();
                self.update_status_cache.clear();
                self.selected_versions.clear();
                self.loading_versions.clear();
                self.cached_versions.clear();

                let load_task = Task::done(ModsMessage::ToCore(
                    crate::core::signals::ToCore::LoadLocalMods {
                        channel: settings.channel.clone(),
                        version: Self::version_str(settings.game_version),
                    },
                ));
                // Tambien verificar actualizaciones automaticamente al abrir
                let update_task = Task::perform(async move { Ok::<(), String>(()) }, |_| {
                    ModsMessage::CheckForUpdates
                });

                Task::batch([load_task, update_task])
            }
            ModsMessage::RefreshLocalBackground => Task::done(ModsMessage::ToCore(
                crate::core::signals::ToCore::LoadLocalMods {
                    channel: settings.channel,
                    version: Self::version_str(settings.game_version),
                },
            )),
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

                        // Populate installed_ids with mod IDs from JAR mods and ZIP patches
                        self.installed_ids.clear();

                        // Add mod IDs from JAR mods that have metadata
                        for jar_mod in &self.installed_mods {
                            if let Some(meta) = &jar_mod.metadata {
                                self.installed_ids.insert(meta.mod_id.clone());
                            }
                        }

                        // Add remote IDs from ZIP patches
                        for patch in &self.patch_mods {
                            if let Some(remote_id) = &patch.remote_id {
                                self.installed_ids.insert(remote_id.clone());
                            }
                        }

                        // Clear installing state for any mod now confirmed installed
                        self.installing_ids.retain(|id| !self.installed_ids.contains(id));
                        self.installing_mods.retain(|id, _| !self.installed_ids.contains(id));

                        // Pedir miniaturas para los instalados que tengan logo_url
                        let mut tasks = Vec::new();
                        for m in &self.installed_mods {
                            if let Some(meta) = &m.metadata {
                                if let Some(logo) = &meta.logo_url {
                                    if resources.global_thumbnails.get(&meta.mod_id).is_none() {
                                        let url = logo.clone();
                                        let id = meta.mod_id.clone();
                                        tasks.push(Task::perform(
                                            async move {
                                                let res = crate::util::image_cache::load_image_bytes(&url).await;
                                                (
                                                    id,
                                                    res.map(image::Handle::from_bytes)
                                                       .map_err(|e: anyhow::Error| e.to_string()),
                                                )
                                            },
                                            |(id, res)| ModsMessage::ImageLoaded(id, res),
                                        ));
                                    }
                                }
                            }
                        }

                        for p in &self.patch_mods {
                            if let (Some(rid), Some(logo)) = (&p.remote_id, &p.logo_url) {
                                if resources.global_thumbnails.get(rid).is_none() {
                                    let url = logo.clone();
                                    let id = rid.clone();
                                    tasks.push(Task::perform(
                                        async move {
                                            let res = crate::util::image_cache::load_image_bytes(&url).await;
                                            (
                                                id,
                                                res.map(image::Handle::from_bytes)
                                                   .map_err(|e: anyhow::Error| e.to_string()),
                                            )
                                        },
                                        |(id, res)| ModsMessage::ImageLoaded(id, res),
                                    ));
                                }
                            }
                        }

                        if !tasks.is_empty() {
                            return Task::batch(tasks);
                        }
                    }
                    Err(e) => {
                        if self.is_open {
                            self.error = Some(format!("Error: {}", e));
                        }
                    }
                }
                Task::none()
            }
            ModsMessage::ToggleLocal(mod_info) => {
                self.loading = true;
                Task::done(ModsMessage::ToCore(
                    crate::core::signals::ToCore::ToggleMod(mod_info.name, !mod_info.enabled),
                ))
            }
            ModsMessage::DeleteLocal(mod_info) => {
                self.loading = true;
                Task::done(ModsMessage::ToCore(
                    crate::core::signals::ToCore::UninstallMod(mod_info.name),
                ))
            }
            ModsMessage::OpenFolder => {
                let p = crate::game::GamePaths::new(base_dir);
                let v = Self::version_str(settings.game_version);
                crate::util::open_path(p.mods_dir(&settings.channel, &v));
                Task::none()
            }
            ModsMessage::OpenPatchFolder => {
                let p = crate::game::GamePaths::new(base_dir);
                let v = Self::version_str(settings.game_version);
                crate::util::open_path(p.core_patches_dir(&settings.channel, &v));
                Task::none()
            }
            ModsMessage::OpenJarFolder => {
                let p = crate::game::GamePaths::new(base_dir);
                let v = Self::version_str(settings.game_version);
                crate::util::open_path(p.mods_dir(&settings.channel, &v));
                Task::none()
            }
            ModsMessage::SwitchTab(tab) => {
                self.current_tab = tab;
                if self.current_tab == ModTab::Browse && self.remote_mods.is_empty() {
                    return Task::done(ModsMessage::ToCore(
                        crate::core::signals::ToCore::SearchMods {
                            query: self.search_query.clone(),
                            offset: self.current_page * self.page_size,
                            limit: self.page_size,
                        },
                    ));
                }
                Task::none()
            }
            ModsMessage::SearchChanged(q) => {
                self.search_query = q;
                Task::none()
            }
            ModsMessage::SearchSubmit => {
                self.current_page = 0;
                Task::done(ModsMessage::ToCore(
                    crate::core::signals::ToCore::SearchMods {
                        query: self.search_query.clone(),
                        offset: self.current_page * self.page_size,
                        limit: self.page_size,
                    },
                ))
            }
            ModsMessage::NextPage => {
                if (self.current_page + 1) * self.page_size < self.total_results {
                    self.current_page += 1;
                    return Task::done(ModsMessage::ToCore(
                        crate::core::signals::ToCore::SearchMods {
                            query: self.search_query.clone(),
                            offset: self.current_page * self.page_size,
                            limit: self.page_size,
                        },
                    ));
                }
                Task::none()
            }
            ModsMessage::PrevPage => {
                if self.current_page > 0 {
                    self.current_page -= 1;
                    return Task::done(ModsMessage::ToCore(
                        crate::core::signals::ToCore::SearchMods {
                            query: self.search_query.clone(),
                            offset: self.current_page * self.page_size,
                            limit: self.page_size,
                        },
                    ));
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
                        let mut tasks = Vec::new();
                        for m in &self.remote_mods {
                            if let Some(logo) = &m.logo_url {
                                if resources.global_thumbnails.get(&m.id).is_none() {
                                    let url = logo.clone();
                                    let id = m.id.clone();
                                    tasks.push(Task::perform(
                                        async move {
                                            let res = crate::util::image_cache::load_image_bytes(&url).await;
                                            (
                                                id,
                                                res.map(image::Handle::from_bytes)
                                                   .map_err(|e: anyhow::Error| e.to_string()),
                                            )
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
                if let Ok(h) = res {
                    resources.global_thumbnails.insert(id, h);
                }
                Task::none()
            }
            ModsMessage::VersionSelected { mod_id, file_id } => {
                // Para la pestana Browse
                self.selected_versions.insert(mod_id, file_id);
                Task::none()
            }
            ModsMessage::InstallMod(remote_mod, specific_file_id) => {
                let id_clone = remote_mod.id.clone();
                self.installing_ids.insert(id_clone.clone());
                self.installing_mods
                    .insert(id_clone.clone(), remote_mod.clone());

                let target_file = if let Some(fid) = specific_file_id {
                    if let Some(cached) = self.cached_versions.get(&remote_mod.id) {
                        cached.iter().find(|f| f.file_id == fid).cloned()
                    } else {
                        remote_mod
                            .latest_files
                            .iter()
                            .find(|f| f.file_id == fid)
                            .cloned()
                    }
                } else {
                    let current_game_ver = if settings.game_version == 0 {
                        "latest".to_string()
                    } else {
                        settings.game_version.to_string()
                    };

                    let files = if let Some(cached) = self.cached_versions.get(&remote_mod.id) {
                        cached
                    } else {
                        &remote_mod.latest_files
                    };

                    files
                        .iter()
                        .find(|f| {
                            current_game_ver == "latest"
                                || f.game_versions.contains(&current_game_ver)
                        })
                        .or_else(|| files.first())
                        .cloned()
                };

                if let Some(file) = target_file {
                    let request = crate::game::mods::ModInstallationRequest {
                        mod_id: remote_mod.id.clone(),
                        mod_name: remote_mod.name.clone(),
                        remote_id: Some(remote_mod.id.clone()),
                        file_id: Some(file.file_id.clone()),
                        file_url: file.download_url.clone(),
                        provider: Some(remote_mod.provider),
                        summary: Some(remote_mod.summary.clone()),
                        logo_url: remote_mod.logo_url.clone(),
                    };
                    return Task::done(ModsMessage::ToCore(
                        crate::core::signals::ToCore::InstallMod(request),
                    ));
                } else {
                    self.installing_ids.remove(&id_clone);
                    self.error = Some("Selected version invalid".to_string());
                    Task::none()
                }
            }
            ModsMessage::OpenModPage(url) => {
                let _ = open::that(url);
                Task::none()
            }
            ModsMessage::ModInstalled(res, id) => {
                self.installing_ids.remove(&id);
                let installing_mod = self.installing_mods.remove(&id);
                match res {
                    Ok(fnm) => {
                        self.installed_ids.insert(id.clone());
                        // Guardar metadatos si tenemos informacion del mod
                        let meta = if let Some(gen_mod) = installing_mod {
                            let file_id_to_save =
                                if let Some(fid) = self.selected_versions.get(&gen_mod.id) {
                                    fid.clone()
                                } else {
                                    // Intento de fallback al primero de latest_files si no hay seleccion explicita
                                    gen_mod
                                        .latest_files
                                        .first()
                                        .map(|f| f.file_id.clone())
                                        .unwrap_or_default()
                                };

                            Some(InstalledModMetadata {
                                file_name: fnm.clone(),
                                mod_name: gen_mod.name.clone(),
                                provider: gen_mod.provider,
                                mod_id: gen_mod.id.clone(),
                                file_id: file_id_to_save,
                                enabled: true,
                                summary: Some(gen_mod.summary.clone()),
                                logo_url: gen_mod.logo_url.clone(),
                                install_date: chrono::Utc::now(),
                                update_available: None,
                            })
                        } else {
                            None
                        };

                        if let Some(ref meta) = meta {
                            let bd = base_dir.clone();
                            let sc = settings.channel.clone();
                            let sv = if settings.game_version == 0 {
                                "latest".to_string()
                            } else {
                                settings.game_version.to_string()
                            };
                            let meta_clone = meta.clone();

                            tokio::spawn(async move {
                                let mut current =
                                    crate::game::mods::load_manifest(&bd, &sc, &sv).await;
                                current.retain(|m| m.mod_id != meta_clone.mod_id);
                                current.push(meta_clone);
                                let _ =
                                    crate::game::mods::save_manifest(&bd, &sc, &sv, &current).await;
                            });
                        }

                        let (bdc, sc, sv) = (
                            base_dir.clone(),
                            settings.channel.clone(),
                            if settings.game_version == 0 {
                                "latest".to_string()
                            } else {
                                settings.game_version.to_string()
                            },
                        );
                        let (md, _) = tokio::task::block_in_place(|| {
                            futures::executor::block_on(crate::game::mods::ensure_mod_dirs(
                                &bdc, &sc, &sv,
                            ))
                        });
                        let fp = md.join(&fnm);
                        if fnm.ends_with(".zip") && crate::game::zip_mods::is_patch_mod(&fp).0 {
                            Task::done(ModsMessage::InstallZipPatch(fp, meta))
                        } else {
                            Task::done(ModsMessage::RefreshLocalBackground)
                        }
                    }
                    Err(e) => {
                        self.error = Some(format!("Installation Failed: {}", e));
                        Task::none()
                    }
                }
            }
            ModsMessage::InstallZipPatch(zp, meta) => {
                // Para archivos ZIP locales, usar la ruta como file_url
                // El ModsService detectará que es una ruta local y la manejará adecuadamente
                let file_name = zp
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();

                let request = ModInstallationRequest {
                    mod_id: meta.as_ref().map(|m| m.mod_id.clone()).unwrap_or_else(|| file_name.clone()),
                    mod_name: meta.as_ref().map(|m| m.mod_name.clone()).unwrap_or_else(|| file_name.clone()),
                    remote_id: meta.as_ref().map(|m| m.mod_id.clone()),
                    file_id: meta.as_ref().map(|m| m.file_id.clone()),
                    file_url: Some(zp.to_string_lossy().to_string()), // Ruta local como URL
                    provider: meta.as_ref().map(|m| m.provider),
                    summary: meta.as_ref().and_then(|m| m.summary.clone()),
                    logo_url: meta.as_ref().and_then(|m| m.logo_url.clone()),
                };

                // Enviar por el canal centralizado
                Task::done(ModsMessage::ToCore(
                    crate::core::signals::ToCore::InstallMod(request),
                ))
            }
            ModsMessage::ToggleZipPatch(id, en) => {
                self.loading = true;
                Task::done(ModsMessage::ToCore(
                    crate::core::signals::ToCore::ToggleZipPatch(id, en),
                ))
            }
            ModsMessage::UninstallZipPatch(id, _) => {
                self.loading = true;
                Task::done(ModsMessage::ToCore(
                    crate::core::signals::ToCore::UninstallMod(id),
                ))
            }
            ModsMessage::PatchOperationFinished(res) => {
                self.loading = false;
                println!("[Mods] PatchOperationFinished received, checking file status...");

                match res {
                    Ok(_) => {
                        println!(
                            "[Mods] Patch operation successful, triggering RefreshLocalBackground"
                        );
                        Task::done(ModsMessage::RefreshLocalBackground)
                    }
                    Err(e) => {
                        self.error = Some(format!("Failed: {}", e));
                        Task::none()
                    }
                }
            }
            ModsMessage::CheckForUpdates => {
                self.checking_updates = true;
                self.mods_with_updates.clear();
                Task::done(ModsMessage::ToCore(crate::core::signals::ToCore::CheckForUpdates))
            }
            // Metodo antiguo (Actualizar a la ultima)
            ModsMessage::UpdateMod(mod_info) => {
                // Redirige a UpdateModToVersion pasando "latest" logic implicitamente
                // O mejor, implementamos una llamada a UpdateModToVersion con None file_id logic interna
                // Para simplificar, este boton busca la ultima version compatible automaticamente.
                if let Some(meta) = &mod_info.metadata {
                    self.installing_ids.insert(meta.mod_id.clone()); // Spinner visual
                    let mod_info_clone = mod_info.clone();
                    // Usamos UpdateModToVersion con un File ID vacio que indica "buscar ultimo"
                    return Task::done(ModsMessage::UpdateModToVersion(
                        mod_info_clone,
                        String::new(),
                        false, // JAR normal
                    ));
                }
                Task::none()
            }
            // NUEVO: Metodo para cambiar a una version especifica (Upgrade/Downgrade)
            ModsMessage::UpdateModToVersion(mod_info, target_file_id, is_patch) => {
                if let Some(meta) = &mod_info.metadata {
                    let mod_id = meta.mod_id.clone();
                    
                    self.loading = true;
                    self.installing_ids.insert(mod_id.clone());

                    if is_patch {
                        // Para patches, usar ToggleZipPatch para reinstalar con nueva versión
                        Task::done(ModsMessage::ToCore(
                            crate::core::signals::ToCore::ToggleZipPatch(mod_id, false), // Desactivar primero
                        ))
                    } else {
                        // Crear ModInstallationRequest para el nuevo archivo
                        let request = ModInstallationRequest {
                            mod_id: mod_id.clone(),
                            mod_name: mod_info.name.clone(),
                            remote_id: Some(mod_id.clone()),
                            file_id: if target_file_id.is_empty() { None } else { Some(target_file_id.clone()) },
                            file_url: None, // Se resolverá en el repositorio
                            provider: Some(meta.provider),
                            summary: meta.summary.clone(),
                            logo_url: meta.logo_url.clone(),
                        };

                        // Enviar por el canal centralizado
                        Task::done(ModsMessage::ToCore(
                            crate::core::signals::ToCore::InstallMod(request),
                        ))
                    }
                } else {
                    Task::none()
                }
            }
            ModsMessage::UpdateModDownloaded(_) => {
                // Este mensaje ya no se usa, ya que la actualización pasa por el sistema centralizado
                self.loading = false;
                Task::none()
            }
            ModsMessage::UpdateModStart(_) => Task::none(),
            ModsMessage::LoadVersions(mod_id) => {
                if self.loading_versions.contains(&mod_id)
                    || self.cached_versions.contains_key(&mod_id)
                {
                    return Task::none();
                }

                self.loading_versions.insert(mod_id.clone());
                Task::done(ModsMessage::ToCore(crate::core::signals::ToCore::LoadVersions(mod_id)))
            }
            // --- VIEWPORT OPTIMIZATION HANDLERS ---
            ModsMessage::ScrollOffsetChanged(offset) => {
                self.scroll_offset = offset;
                Task::none()
            }
            ModsMessage::UpdateViewportHeight(height) => {
                self.viewport_height = height.max(300.0); // Mínimo 300px
                Task::none()
            }
            ModsMessage::ToCore(_) => Task::none(),
        }
    }

    pub fn view<'a>(
        &'a self,
        localization: &'a rustale_shared::lang::Localization,
        ws: Size,
        resources: &'a crate::ui::resources::UiResources,
        ctx: theme::UIContext,
    ) -> Element<'a, ModsMessage, Theme, Renderer> {
        let is_c = ws.width < 600.0;
        let ts = row![
            theme::magic_button(
                Self::tab_btn(
                    localization.t("mods.installed"),
                    self.current_tab == ModTab::Installed,
                    is_c,
                    ctx
                )
                .on_press(ModsMessage::SwitchTab(ModTab::Installed))
                .into(),
                ctx
            ),
            theme::magic_button(
                Self::tab_btn(
                    localization.t("mods.browse"),
                    self.current_tab == ModTab::Browse,
                    is_c,
                    ctx
                )
                .on_press(ModsMessage::SwitchTab(ModTab::Browse))
                .into(),
                ctx
            )
        ]
        .spacing(10);

        let cnt = match self.current_tab {
            ModTab::Installed => self.view_installed(localization, resources, ctx), // Update call
            ModTab::Browse => self.view_browse(localization, is_c, resources, ctx), // Update call
        };

        let (mw, mh) = (
            if ws.width < 850.0 {
                Length::Fill
            } else {
                Length::Fixed(800.0)
            },
            if ws.height < 650.0 {
                Length::Fill
            } else {
                Length::Fixed(600.0)
            },
        );

        let view_content = theme::magic_column(
            vec![
                container(ts)
                    .padding([0, theme::STANDARD_PADDING as u16])
                    .style(move |t| theme::container_style_transparent(&ctx.palette, t))
                    .into(),
                theme::page_container(cnt).into(),
            ],
            ctx,
        );

        theme::modal_shell(
            localization.t("mods.title").to_uppercase(),
            view_content,
            None,
            ModsMessage::Close,
            ctx,
        )
        .width(mw)
        .height(mh)
        .into()
    }

    fn view_installed<'a>(
        &'a self,
        localization: &'a rustale_shared::lang::Localization,
        resources: &'a crate::ui::resources::UiResources,
        ctx: theme::UIContext,
    ) -> Element<'a, ModsMessage, Theme, Renderer> {
        let palette = ctx.palette;

        let check_updates_btn = button(if self.checking_updates {
            theme::text_caption("Checking...", ctx)
        } else {
            theme::text_caption(localization.t("settings.check_updates"), ctx)
        })
        .on_press_maybe(if !self.checking_updates {
            Some(ModsMessage::CheckForUpdates)
        } else {
            None
        })
        .style(move |t: &Theme, s| theme::secondary_button_style(&palette, t, s))
        .padding(5);

        if self.installed_mods.is_empty() && self.patch_mods.is_empty() {
            return container(theme::text_body(localization.t("mods.no_mods"), ctx))
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .style(move |t| theme::container_style_transparent(&palette, t))
                .into();
        }

        let mut content_items = Vec::new();

        if !self.patch_mods.is_empty() {
            let h = row![
                theme::text_title(localization.t("mods.core_patches"), ctx),
                Space::new().width(Length::Fill),
                theme::magic_button(
                    button(
                        row![
                            theme::svg(
                                svg(util::svg_handle(util::icons::FOLDER))
                                    .width(14)
                                    .height(14)
                                    .style(move |t: &Theme, s| theme::svg_accent(&palette, t, s)),
                                ctx
                            ),
                            theme::text_caption(localization.t("mods.open_folder"), ctx)
                        ]
                        .spacing(5)
                        .align_y(Alignment::Center)
                    )
                    .on_press(ModsMessage::OpenPatchFolder)
                    .style(move |t: &Theme, s| theme::secondary_button_style(&palette, t, s))
                    .padding(5)
                    .into(),
                    ctx,
                )
            ]
            .align_y(Alignment::Center);

            let mut pl = column![Element::from(h)].spacing(10);
            for p in &self.patch_mods {
                // AHORA: usar el remote_id (el ID de CurseForge)
                let has_update = if let Some(rid) = &p.remote_id {
                    self.mods_with_updates.contains(rid)
                } else {
                    false
                };
                pl = pl.push(self.view_patch_row(p, has_update, localization, resources, ctx));
            }
            content_items.push(theme::magic_container(pl.into(), ctx));
        }

        let hj = row![
            theme::text_title(localization.t("mods.jar_mods"), ctx),
            Space::new().width(Length::Fill),
            theme::magic_button(
                button(
                    row![
                        theme::svg(
                            svg(util::svg_handle(util::icons::FOLDER))
                                .width(14)
                                .height(14)
                                .style(move |t: &Theme, s| theme::svg_accent(&palette, t, s)),
                            ctx
                        ),
                        theme::text_caption(localization.t("mods.open_folder"), ctx)
                    ]
                    .spacing(5)
                    .align_y(Alignment::Center)
                )
                .on_press(ModsMessage::OpenJarFolder)
                .style(move |t: &Theme, s| theme::secondary_button_style(&palette, t, s))
                .padding(5)
                .into(),
                ctx,
            ),
            theme::magic_button(check_updates_btn.into(), ctx)
        ]
        .spacing(10)
        .align_y(Alignment::Center);

        let mut ml = column![Element::from(hj)].spacing(10);

        if !self.installed_mods.is_empty() {
            // --- VIEWPORT OPTIMIZATION ---
            const MOD_ROW_HEIGHT: f32 = 80.0; // Altura estimada de cada fila de mod
            let (start_idx, end_idx) =
                self.get_visible_range(MOD_ROW_HEIGHT, self.installed_mods.len(), 2);

            // Espacio arriba para mantener scroll
            if start_idx > 0 {
                let space_above = Space::new()
                    .width(Length::Fill)
                    .height(Length::Fixed(start_idx as f32 * MOD_ROW_HEIGHT));
                ml = ml.push(space_above);
            }

            // Solo renderizar mods visibles
            for (_, m) in self
                .installed_mods
                .iter()
                .enumerate()
                .skip(start_idx)
                .take(end_idx - start_idx)
            {
                let has_update = if let Some(meta) = &m.metadata {
                    self.mods_with_updates.contains(&meta.mod_id)
                } else {
                    false
                };
                ml = ml.push(self.view_installed_row(m, has_update, localization, resources, ctx));
            }

            // Espacio abajo para mantener scroll total
            if end_idx < self.installed_mods.len() {
                let space_below = Space::new().width(Length::Fill).height(Length::Fixed(
                    (self.installed_mods.len() - end_idx) as f32 * MOD_ROW_HEIGHT,
                ));
                ml = ml.push(space_below);
            }
        } else if self.patch_mods.is_empty() {
            ml = ml.push(theme::text_caption(localization.t("mods.no_mods_jar"), ctx));
        }
        content_items.push(theme::magic_container(ml.into(), ctx));

        theme::magic_scrollable(
            scrollable(theme::magic_column(content_items, ctx))
                .id(Id::new(MODS_SCROLL_ID))
                .on_scroll(|viewport| {
                    ModsMessage::ScrollOffsetChanged(viewport.absolute_offset().y)
                })
                .height(Length::Fill)
                .style(move |t: &Theme, s| theme::scrollable_style(&palette, t, s))
                .into(),
            ctx,
        )
        .into()
    }

    fn view_installed_row<'a>(
        &'a self,
        mi: &'a ModInfo,
        has_update: bool,
        localization: &'a rustale_shared::lang::Localization,
        resources: &'a crate::ui::resources::UiResources,
        ctx: theme::UIContext,
    ) -> Element<'a, ModsMessage, Theme, Renderer> {
        let palette = ctx.palette;
        let mut actions = Vec::new();

        let is_installing = if let Some(meta) = &mi.metadata {
            self.installing_ids.contains(&meta.mod_id)
        } else {
            false
        };

        let display_name = if let Some(meta) = &mi.metadata {
            &meta.mod_name
        } else {
            &mi.name
        };

        // --------------------------------------------------------
        // SECCION CENTRAL: Nombre + Dropdown de versiones + Summary
        // --------------------------------------------------------
        let mut center_info = column![theme::text_body(display_name, ctx)]
            .spacing(4)
            .width(Length::Fill);

        if let Some(meta) = &mi.metadata {
            if let Some(summary) = &meta.summary {
                center_info = center_info.push(theme::text_caption(summary, ctx));
            }
        }

        // Si tenemos metadatos (fue instalado via gestor), mostramos selector
        if let Some(meta) = &mi.metadata {
            let mod_id = &meta.mod_id;

            // Check si las versiones estan cargadas en memoria o cargando
            let cached = self.cached_versions.get(mod_id);
            let is_loading_versions = self.loading_versions.contains(mod_id);

            if is_installing {
                center_info = center_info.push(theme::text_caption(
                    localization.t("launcher.status.downloading"),
                    ctx,
                ));
            } else if is_loading_versions {
                center_info = center_info.push(theme::text_caption("Fetching versions...", ctx));
            } else if let Some(files) = cached {
                // YA TENEMOS VERSIONES: Mostrar Dropdown
                let selected = files.iter().find(|f| f.file_id == meta.file_id).cloned();

                let pick = pick_list(files.as_slice(), selected, move |f| {
                    ModsMessage::UpdateModToVersion(mi.clone(), f.file_id.clone(), false)
                })
                .text_size(12)
                .placeholder("Select version...")
                .padding(5)
                .width(Length::Fixed(180.0)) // Ancho fijo para consistencia
                .style(move |t, s| theme::pick_list_style(&palette, t, s))
                .menu_style(move |t| theme::menu_style(&palette, t));

                center_info = center_info.push(theme::magic_pick_list_with_menu(pick.into(), ctx));
            } else {
                // NO TENEMOS VERSIONES: Boton Lazy "Check Versions"
                let load_btn = button(
                    row![
                        theme::svg(
                            svg(util::svg_handle(util::icons::REFRESH))
                                .width(10)
                                .height(10)
                                .style(move |_, _| svg::Style {
                                    color: Some(palette.text_secondary)
                                }),
                            ctx
                        ),
                        theme::text_micro(localization.t("mods.load_versions"), ctx)
                    ]
                    .spacing(5)
                    .align_y(Alignment::Center),
                )
                .on_press(ModsMessage::LoadVersions(mod_id.clone()))
                .style(move |t, s| theme::ghost_button_style(&palette, t, s))
                .padding(4);

                center_info = center_info.push(theme::magic_button(load_btn.into(), ctx));
            }
        } else {
            // Mod manual (sin metadata)
            center_info = center_info.push(theme::text_caption(localization.t("mods.manual_install"), ctx));
        }

        // --------------------------------------------------------
        // ACCIONES DERECHA
        // --------------------------------------------------------

        if is_installing {
            // Mientras instala no permitimos acciones
            actions.push(theme::text_micro("WAIT...", ctx).into());
        } else {
            // Boton UPDATE funcional cuando hay actualizacion disponible
            if has_update {
                let update_btn = theme::magic_button(
                    button(theme::text_micro("UPDATE", ctx))
                        .on_press(ModsMessage::UpdateMod(mi.clone()))
                        .style(move |t: &Theme, s| theme::success_button_style(&palette, t, s))
                        .padding([4, 8])
                        .into(),
                    ctx,
                );
                actions.push(update_btn.into());
            }

            // Toggle Enable/Disable
            let tb = if mi.enabled {
                theme::magic_button(
                    button(theme::text_caption(localization.t("mods.disable"), ctx))
                        .style(move |t: &Theme, s| theme::secondary_button_style(&palette, t, s))
                        .on_press(ModsMessage::ToggleLocal(mi.clone()))
                        .into(),
                    ctx,
                )
            } else {
                theme::magic_button(
                    button(theme::text_caption(localization.t("mods.enable"), ctx))
                        .style(move |t: &Theme, s| theme::primary_button_style(&palette, t, s))
                        .on_press(ModsMessage::ToggleLocal(mi.clone()))
                        .into(),
                    ctx,
                )
            };
            actions.push(tb.into());

            // Delete
            let db = theme::magic_button(
                button(theme::svg(
                    svg(util::svg_handle(util::icons::TRASH))
                        .width(14)
                        .height(14)
                        .style(move |_, _| iced::widget::svg::Style {
                            color: Some(crate::theme::svg_icon_color(&palette)),
                        }),
                    ctx,
                ))
                .on_press(ModsMessage::DeleteLocal(mi.clone()))
                .style(move |t: &Theme, s| theme::danger_button_style(&palette, t, s))
                .padding(8)
                .into(),
                ctx,
            );
            actions.push(db.into());
        }

        theme::magic_container(
            theme::list_item_row(
                row![
                    // Miniatura o Icono (PUZZLE para JAR mods)
                    Element::from(if let Some(meta) = &mi.metadata {
                        if let Some(h) = resources.global_thumbnails.peek(&meta.mod_id) {
                            theme::magic_image::<ModsMessage>(
                                image(h.clone())
                                    .width(40)
                                    .height(40)
                                    .content_fit(ContentFit::Cover)
                                    .into(),
                                ctx,
                            )
                        } else {
                            theme::svg(
                                svg(util::svg_handle(util::icons::PUZZLE))
                                    .width(20)
                                    .height(20)
                                    .style(|_, _| iced::widget::svg::Style {
                                        color: Some(Color::BLACK),
                                    }),
                                ctx,
                            )
                        }
                    } else {
                        theme::svg(
                            svg(util::svg_handle(util::icons::PUZZLE))
                                .width(20)
                                .height(20)
                                .style(|_, _| iced::widget::svg::Style {
                                    color: Some(Color::BLACK),
                                }),
                            ctx,
                        )
                    }),
                    // Columna central con Nombre + Selector
                    center_info,
                    // Estado visual pequeno (opcional)
                    theme::text_micro(
                        if mi.enabled {
                            localization.t("mods.active")
                        } else {
                            localization.t("mods.disabled")
                        },
                        ctx
                    ),
                ]
                .spacing(15)
                .align_y(Alignment::Center)
                .into(),
                actions,
                ctx,
            )
            .into(),
            ctx,
        )
    }

    fn view_browse<'a>(
        &'a self,
        localization: &'a rustale_shared::lang::Localization,
        is_c: bool,
        resources: &'a crate::ui::resources::UiResources,
        ctx: theme::UIContext,
    ) -> Element<'a, ModsMessage, Theme, Renderer> {
        let palette = ctx.palette;
        let sb = row![
            theme::magic_text_input(
                text_input(localization.t("mods.search"), &self.search_query)
                    .on_input(ModsMessage::SearchChanged)
                    .on_submit(ModsMessage::SearchSubmit)
                    .padding(10)
                    .style(move |t: &Theme, s| theme::text_input_style(&palette, t, s))
                    .width(Length::Fill)
                    .into(),
                ctx,
            ),
            theme::magic_button(
                button(theme::text_body(localization.t("mods.browse"), ctx))
                    .on_press(ModsMessage::SearchSubmit)
                    .style(move |t: &Theme, s| theme::primary_button_style(&palette, t, s))
                    .padding(10)
                    .into(),
                ctx,
            )
        ]
        .spacing(10);
        let c: Element<'a, ModsMessage, Theme, Renderer> = if self.loading {
            container(theme::text_body(localization.t("mods.loading"), ctx))
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .height(Length::Fill)
                .style(move |t| theme::container_style_transparent(&palette, t))
                .into()
        } else if let Some(err) = &self.error {
            container(theme::text_body(err, ctx))
                .center_x(Length::Fill)
                .style(move |t| theme::container_style_transparent(&palette, t))
                .into()
        } else {
            // --- VIEWPORT OPTIMIZATION ---
            const MOD_CARD_HEIGHT: f32 = 120.0; // Altura estimada de cada card de mod
            let (start_idx, end_idx) =
                self.get_visible_range(MOD_CARD_HEIGHT, self.remote_mods.len(), 1);

            let mut l = column([]).spacing(10);

            // Espacio arriba para mantener scroll
            if start_idx > 0 {
                let space_above = Space::new()
                    .width(Length::Fill)
                    .height(Length::Fixed(start_idx as f32 * MOD_CARD_HEIGHT));
                l = l.push(space_above);
            }

            // Solo renderizar mods visibles
            for m in self
                .remote_mods
                .iter()
                .skip(start_idx)
                .take(end_idx - start_idx)
            {
                l = l.push(self.view_remote_card(m, localization, resources, ctx));
            }

            // Espacio abajo para mantener scroll total
            if end_idx < self.remote_mods.len() {
                let space_below = Space::new().width(Length::Fill).height(Length::Fixed(
                    (self.remote_mods.len() - end_idx) as f32 * MOD_CARD_HEIGHT,
                ));
                l = l.push(space_below);
            }

            theme::magic_container(
                container(theme::magic_scrollable(
                    scrollable(l)
                        .on_scroll(|viewport| {
                            ModsMessage::ScrollOffsetChanged(viewport.absolute_offset().y)
                        })
                        .height(Length::Fill)
                        .style(move |t: &Theme, s| theme::scrollable_style(&palette, t, s))
                        .into(),
                    ctx,
                ))
                .padding(0)
                .style(move |t| theme::container_style_transparent(&palette, t))
                .into(),
                ctx,
            )
            .into()
        };
        let pr = button(theme::text_body(localization.t("mods.prev"), ctx))
            .style(move |t: &Theme, s| theme::secondary_button_style(&palette, t, s))
            .padding(if is_c { 6 } else { 10 });
        let pr = if self.current_page > 0 {
            pr.on_press(ModsMessage::PrevPage)
        } else {
            pr
        };
        let pg = row![
            theme::magic_button(pr.into(), ctx),
            theme::text_body(
                &localization
                    .t("mods.page")
                    .replace("{0}", &(self.current_page + 1).to_string())
                    .replace("{1}", "?"), // Remote page count not easily known here
                ctx
            ),
            theme::magic_button(
                button(theme::text_body(localization.t("mods.next"), ctx))
                    .on_press(ModsMessage::NextPage)
                    .style(move |t: &Theme, s| theme::secondary_button_style(&palette, t, s))
                    .padding(if is_c { 6 } else { 10 })
                    .into(),
                ctx,
            )
        ]
        .spacing(if is_c { 10 } else { 20 })
        .align_y(Alignment::Center)
        .width(Length::Fill);
        theme::magic_column(vec![sb.into(), c.into(), pg.into()], ctx).into()
    }

    fn view_remote_card<'a>(
        &'a self,
        cf: &'a GenericMod,
        localization: &'a rustale_shared::lang::Localization,
        resources: &'a crate::ui::resources::UiResources,
        ctx: theme::UIContext,
    ) -> Element<'a, ModsMessage, Theme, Renderer> {
        let (isi, isin) = (
            self.installing_ids.contains(&cf.id),
            self.installed_ids.contains(&cf.id),
        );

        // 1. Obtener archivos disponibles para este mod
        // Usar cache si existe, sino los archivos iniciales de la busqueda
        let files = if let Some(cached) = self.cached_versions.get(&cf.id) {
            cached
        } else {
            &cf.latest_files
        };

        // 2. Verificar si esta cargando versiones
        let is_loading = self.loading_versions.contains(&cf.id);

        // 3. Buscar cual esta seleccionado actualmente (o default) - PATRoN DE SETTINGS
        let current_game_ver = if self.temp_settings.game_version == 0 {
            "latest".to_string()
        } else {
            self.temp_settings.game_version.to_string()
        };

        let selected_id_in_map = self.selected_versions.get(&cf.id);

        let selected_file = if is_loading {
            // Durante carga, seleccionar el primer archivo (como en settings)
            files.first()
        } else if let Some(fid) = selected_id_in_map {
            // Si hay uno seleccionado manualmente
            files.iter().find(|f| &f.file_id == fid)
        } else {
            // Default inteligente: El mas nuevo compatible
            files
                .iter()
                .find(|f| {
                    current_game_ver == "latest" || f.game_versions.contains(&current_game_ver)
                })
                .or(files.first())
        };

        // 4. Crear el Dropdown (PickList)
        // Si las versiones completas no estan cacheadas, mostrar un boton-dropdown falso
        // que dispara LoadVersions al hacer click (sin necesidad de boton separado).
        let is_full_list_loaded = self.cached_versions.contains_key(&cf.id);
        let needs_load = !is_full_list_loaded && !is_loading;

        let version_selector: Element<'a, ModsMessage, Theme, Renderer> = if is_loading {
            // Mostrar "Fetching versions..." mientras carga
            row![theme::text_caption("Fetching versions...", ctx)].into()
        } else if needs_load {
            // Versiones no cargadas: boton con aspecto de dropdown que carga al hacer click
            let label = selected_file
                .map(|f| f.to_string())
                .unwrap_or_else(|| "Select version...".to_string());

            theme::magic_button(
                button(
                    row![
                        theme::text_micro(&label, ctx),
                        Space::new().width(Length::Fill),
                        theme::text_micro("▼", ctx),
                    ]
                    .spacing(5)
                    .align_y(Alignment::Center)
                    .width(Length::Fixed(220.0)),
                )
                .on_press(ModsMessage::LoadVersions(cf.id.clone()))
                .style(move |t, s| theme::secondary_button_style(&ctx.palette, t, s))
                .padding(10)
                .width(Length::Fixed(240.0))
                .into(),
                ctx,
            )
            .into()
        } else if files.is_empty() {
            // Fallback visual si la API no trajo archivos
            theme::text_caption("No versions found", ctx).into()
        } else {
            // Versiones ya cargadas: pick_list real
            let pick = pick_list(
                files.as_slice(),
                selected_file,
                move |f| ModsMessage::VersionSelected {
                    mod_id: cf.id.clone(),
                    file_id: f.file_id.clone(),
                },
            )
            .text_size(12)
            .placeholder("Select version...")
            .padding(10)
            .width(Length::Fixed(240.0))
            .style(move |t, s| theme::pick_list_style(&ctx.palette, t, s))
            .menu_style(move |t| theme::menu_style(&ctx.palette, t));

            container(theme::magic_pick_list_with_menu(pick.into(), ctx))
                .width(Length::Fixed(240.0))
                .into()
        };
        // 4. Modificar el Boton de Instalar para usar la seleccion
        let install_action = if isi {
            button(theme::text_caption(
                localization.t("launcher.status.downloading"),
                ctx,
            ))
            .style(move |t: &Theme, s| theme::ghost_button_style(&ctx.palette, t, s))
        } else if isin {
            button(theme::text_caption(localization.t("mods.installed"), ctx))
                .style(move |t: &Theme, s| theme::success_button_style(&ctx.palette, t, s))
        } else {
            // AQUI ESTA LA MAGIA: Pasamos selected_file.file_id
            let target_id = selected_file.map(|f| f.file_id.clone());

            button(theme::text_caption(localization.t("mods.install"), ctx))
                .on_press(ModsMessage::InstallMod(cf.clone(), target_id))
                .style(move |t: &Theme, s| theme::primary_button_style(&ctx.palette, t, s))
        }
        .padding(8);

        let th: Element<'a, ModsMessage, Theme, Renderer> =
            if let Some(h) = resources.global_thumbnails.peek(&cf.id) {
                theme::magic_image(
                    image(h.clone())
                        .width(50)
                        .height(50)
                        .content_fit(ContentFit::Cover)
                        .into(),
                    ctx,
                )
                .into()
            } else {
                container(Space::new())
                    .width(50)
                    .height(50)
                    .style(move |t: &Theme| theme::card_style(&ctx.palette, t))
                    .into()
            };

        let card_content = theme::list_item_row(
            row![
                th,
                column![
                    theme::text_body(&cf.name, ctx),
                    // Anadimos selector aqui debajo del titulo
                    version_selector,
                    theme::text_caption(&cf.summary, ctx),
                    theme::text_micro(
                        format!("{}: {:.0}", localization.t("mods.downloads"), cf.downloads),
                        ctx
                    )
                ]
                .spacing(4)
                .width(Length::Fill)
            ]
            .spacing(15)
            .align_y(Alignment::Center)
            .into(),
            vec![theme::magic_button(install_action.into(), ctx)],
            ctx,
        );

        // Hacer la card cliqueable para abrir la pagina del mod
        let clickable_card = button(card_content)
            .on_press(ModsMessage::OpenModPage(cf.website_url.clone()))
            .style(move |t: &Theme, s| theme::ghost_button_style(&ctx.palette, t, s))
            .padding(0);

        theme::magic_container(clickable_card.into(), ctx)
    }

    fn tab_btn<'a>(
        l: &'a str,
        a: bool,
        c: bool,
        ctx: theme::UIContext,
    ) -> button::Button<'a, ModsMessage, Theme, Renderer> {
        let palette = ctx.palette;
        let btn_content = container(theme::text_body(l, ctx))
            .width(Length::Fill)
            .center_x(Length::Fill)
            .style(move |t| theme::container_style_transparent(&palette, t));

        button(btn_content)
            .width(Length::Fill)
            .padding(if c { 6 } else { 8 })
            .style(move |t: &Theme, s| {
                if a {
                    theme::active_tab_style(&palette, t, s)
                } else {
                    theme::ghost_button_style(&palette, t, s)
                }
            })
    }

    fn view_patch_row<'a>(
        &'a self,
        p: &'a PatchManifest,
        has_update: bool,
        localization: &'a rustale_shared::lang::Localization,
        resources: &'a crate::ui::resources::UiResources,
        ctx: theme::UIContext,
    ) -> Element<'a, ModsMessage, Theme, Renderer> {
        let palette = ctx.palette;
        let mut actions = Vec::new();

        let is_installing = if let Some(rid) = &p.remote_id {
            self.installing_ids.contains(rid)
        } else {
            false
        };

        // Construir un ModInfo temporal para el dropdown
        // Esto permite reusar UpdateModToVersion
        let temp_mi = ModInfo {
            name: p.mod_name.clone(),
            file_name: p.mod_id.clone(), // Identificador de la carpeta del patch
            path: std::path::PathBuf::new(),
            enabled: p.enabled,
            size: 0,
            metadata: p.remote_id.as_ref().map(|rid| InstalledModMetadata {
                file_name: String::new(), // No se usa para el fetch
                mod_name: p.mod_name.clone(),
                provider: p
                    .provider
                    .unwrap_or(crate::game::mods_api::ModProvider::CurseForge),
                mod_id: rid.clone(),
                file_id: p.file_id.clone().unwrap_or_default(),
                enabled: p.enabled,
                summary: p.summary.clone(),
                logo_url: p.logo_url.clone(),
                install_date: p.install_date,
                update_available: None,
            }),
        };

        let mut center_info = column![theme::text_body(&p.mod_name, ctx)]
            .spacing(4)
            .width(Length::Fill);

        if let Some(summary) = &p.summary {
            center_info = center_info.push(theme::text_caption(summary, ctx));
        }

        // Selector de versiones para Patches
        if let Some(meta) = &temp_mi.metadata {
            let mod_id = &meta.mod_id;
            let cached = self.cached_versions.get(mod_id);
            let is_loading_versions = self.loading_versions.contains(mod_id);

            if is_installing {
                center_info = center_info.push(theme::text_caption(
                    localization.t("launcher.status.downloading"),
                    ctx,
                ));
            } else if is_loading_versions {
                center_info = center_info.push(theme::text_caption("Fetching versions...", ctx));
            } else if let Some(files) = cached {
                let selected = files
                    .iter()
                    .find(|f| Some(f.file_id.clone()) == p.file_id)
                    .cloned();

                let temp_mi_clone = temp_mi.clone();
                let pick = pick_list(files.as_slice(), selected, move |f| {
                    ModsMessage::UpdateModToVersion(temp_mi_clone.clone(), f.file_id.clone(), true)
                })
                .text_size(12)
                .placeholder("Select version...")
                .padding(5)
                .width(Length::Fixed(180.0))
                .style(move |t, s| theme::pick_list_style(&palette, t, s))
                .menu_style(move |t| theme::menu_style(&palette, t));

                center_info = center_info.push(theme::magic_pick_list_with_menu(pick.into(), ctx));
            } else {
                let load_btn = button(
                    row![
                        theme::svg(
                            svg(util::svg_handle(util::icons::REFRESH))
                                .width(10)
                                .height(10)
                                .style(move |_, _| svg::Style {
                                    color: Some(palette.text_secondary)
                                }),
                            ctx
                        ),
                        theme::text_micro(localization.t("mods.load_versions"), ctx)
                    ]
                    .spacing(5)
                    .align_y(Alignment::Center),
                )
                .on_press(ModsMessage::LoadVersions(mod_id.clone()))
                .style(move |t, s| theme::ghost_button_style(&palette, t, s))
                .padding(4);

                center_info = center_info.push(theme::magic_button(load_btn.into(), ctx));
            }
        } else {
            center_info = center_info.push(theme::text_caption(localization.t("mods.manual_patch"), ctx));
        }

        // ACCIONES
        if is_installing {
            actions.push(theme::text_micro("WAIT...", ctx).into());
        } else {
            // Boton UPDATE funcional cuando hay actualizacion disponible
            if has_update {
                let update_btn = theme::magic_button(
                    button(theme::text_micro("UPDATE", ctx))
                        .on_press(ModsMessage::UpdateMod(temp_mi.clone()))
                        .style(move |t: &Theme, s| theme::success_button_style(&palette, t, s))
                        .padding([4, 8])
                        .into(),
                    ctx,
                );
                actions.push(update_btn.into());
            }

            let tb = if p.enabled {
                theme::magic_button(
                    button(theme::text_caption(localization.t("mods.disable"), ctx))
                        .style(move |t: &Theme, s| theme::secondary_button_style(&palette, t, s))
                        .on_press(ModsMessage::ToggleZipPatch(p.mod_id.clone(), false))
                        .into(),
                    ctx,
                )
            } else {
                theme::magic_button(
                    button(theme::text_caption(localization.t("mods.enable"), ctx))
                        .style(move |t: &Theme, s| theme::primary_button_style(&palette, t, s))
                        .on_press(ModsMessage::ToggleZipPatch(p.mod_id.clone(), true))
                        .into(),
                    ctx,
                )
            };
            actions.push(tb.into());

            let db = theme::magic_button(
                button(theme::svg(
                    svg(util::svg_handle(util::icons::TRASH))
                        .width(14)
                        .height(14)
                        .style(move |_, _| iced::widget::svg::Style {
                            color: Some(crate::theme::svg_icon_color(&palette)),
                        }),
                    ctx,
                ))
                .on_press(ModsMessage::UninstallZipPatch(
                    p.mod_id.clone(),
                    self.temp_settings.clone(),
                ))
                .style(move |t: &Theme, s| theme::danger_button_style(&palette, t, s))
                .padding(8)
                .into(),
                ctx,
            );
            actions.push(db.into());
        }

        theme::magic_container(
            theme::list_item_row(
                row![
                    // Icono visual (ZIP para patches, o logo si existe)
                    Element::from(if let Some(_logo) = &p.logo_url {
                        if let Some(rid) = &p.remote_id {
                            if let Some(h) = resources.global_thumbnails.peek(rid) {
                                theme::magic_image::<ModsMessage>(
                                    image(h.clone())
                                        .width(40)
                                        .height(40)
                                        .content_fit(ContentFit::Cover)
                                        .into(),
                                    ctx,
                                )
                            } else {
                                theme::svg(
                                    svg(util::svg_handle(util::icons::ZIP))
                                        .width(20)
                                        .height(20)
                                        .style(|_, _| iced::widget::svg::Style {
                                            color: Some(Color::BLACK),
                                        }),
                                    ctx,
                                )
                            }
                        } else {
                            theme::svg(
                                svg(util::svg_handle(util::icons::ZIP))
                                    .width(20)
                                    .height(20)
                                    .style(|_, _| iced::widget::svg::Style {
                                        color: Some(Color::BLACK),
                                    }),
                                ctx,
                            )
                        }
                    } else {
                        theme::svg(
                            svg(util::svg_handle(util::icons::ZIP))
                                .width(20)
                                .height(20)
                                .style(|_, _| iced::widget::svg::Style {
                                    color: Some(Color::BLACK),
                                }),
                            ctx,
                        )
                    }),
                    center_info,
                ]
                .spacing(10)
                .align_y(Alignment::Center)
                .into(),
                actions,
                ctx,
            )
            .into(),
            ctx,
        )
    }
}

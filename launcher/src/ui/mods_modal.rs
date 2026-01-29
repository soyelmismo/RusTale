use crate::config::GameSettings;
use crate::game::mods::{InstalledModMetadata, ModInfo};
use crate::game::mods_api::curseforge::CurseForgeRepository;
use crate::game::mods_api::{GenericMod, ModProvider, ModRepository, SearchResults};
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
        GameSettings,
        Option<InstalledModMetadata>,
    ),
    ToggleZipPatch(String, bool),
    UninstallZipPatch(String, GameSettings),
    PatchOperationFinished(Result<(), String>),
    ModsLoadedComplex(Result<(Vec<ModInfo>, Vec<crate::game::zip_mods::PatchManifest>), String>),
    OpenMods,
    CheckForUpdates,
    UpdatesChecked(
        Result<
            (
                Vec<String>,
                std::collections::HashMap<String, Vec<crate::game::mods_api::GenericFile>>,
            ),
            String,
        >,
    ),
    UpdateMod(ModInfo),
    UpdateModToVersion(ModInfo, String, bool), // (ModInfo, FileID, is_patch)
    UpdateModStart(ModInfo),
    UpdateModDownloaded(Result<(String, ModInfo, String, bool), String>),
    VersionSelected {
        mod_id: String,
        file_id: String,
    },
    LoadVersions(String), // mod_id
    VersionsLoaded(Result<(String, Vec<crate::game::mods_api::GenericFile>), String>),
}

pub struct ModsState {
    pub is_open: bool,
    pub current_tab: ModTab,
    pub search_query: String,
    pub remote_mods: Vec<GenericMod>,
    pub current_page: u32,
    pub total_results: u32,
    pub page_size: u32,
    pub thumbnails: HashMap<String, image::Handle>,
    pub loading: bool,
    pub error: Option<String>,
    pub patch_mods: Vec<PatchManifest>,
    pub installed_mods: Vec<ModInfo>,
    pub temp_settings: GameSettings,
    pub installing_ids: HashSet<String>,
    pub installed_ids: HashSet<String>,
    pub installing_mods: HashMap<String, GenericMod>,
    pub checking_updates: bool,
    pub mods_with_updates: HashSet<String>,
    // Mapa para recordar qué versión seleccionó el usuario en la UI para cada mod (Browse tab)
    pub selected_versions: HashMap<String, String>,
    // Set de mods que están cargando versiones actualmente
    pub loading_versions: HashSet<String>,
    // Cache de versiones cargadas bajo demanda para cada mod
    pub cached_versions: HashMap<String, Vec<crate::game::mods_api::GenericFile>>,
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
            installing_mods: HashMap::new(),
            checking_updates: false,
            mods_with_updates: HashSet::new(),
            selected_versions: HashMap::new(),
            loading_versions: HashSet::new(),
            cached_versions: HashMap::new(),
        }
    }
}

impl ModsState {
    pub fn new() -> Self {
        Self::default()
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
                self.installed_mods.clear();
                self.patch_mods.clear();
                self.installing_ids.clear();
                self.installed_ids.clear();
                // Limpiar cache al reabrir para refrescar datos frescos
                self.mods_with_updates.clear();

                let load_task = self.load_mods_task(base_dir.clone(), settings.clone());
                // También verificar actualizaciones automáticamente al abrir
                let update_task = Task::perform(async move { Ok::<(), String>(()) }, |_| {
                    ModsMessage::CheckForUpdates
                });

                Task::batch([load_task, update_task])
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

                        // Pedir miniaturas para los instalados que tengan logo_url
                        let mut tasks = Vec::new();
                        for m in &self.installed_mods {
                            if let Some(meta) = &m.metadata {
                                if let Some(logo) = &meta.logo_url {
                                    if !self.thumbnails.contains_key(&meta.mod_id) {
                                        let url = logo.clone();
                                        let id = meta.mod_id.clone();
                                        let c = client.clone();
                                        tasks.push(Task::perform(
                                            async move {
                                                (
                                                    id,
                                                    crate::util::image_cache::load_image(&c, &url)
                                                        .await
                                                        .map_err(|e| e.to_string()),
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
                                if !self.thumbnails.contains_key(rid) {
                                    let url = logo.clone();
                                    let id = rid.clone();
                                    let c = client.clone();
                                    tasks.push(Task::perform(
                                        async move {
                                            (
                                                id,
                                                crate::util::image_cache::load_image(&c, &url)
                                                    .await
                                                    .map_err(|e| e.to_string()),
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
                let bd = base_dir.clone();
                let ch = settings.channel.clone();
                let v = if settings.game_version == 0 {
                    "latest".to_string()
                } else {
                    settings.game_version.to_string()
                };
                Task::perform(
                    async move {
                        let _ = crate::game::mods::toggle_mod(&bd, &ch, &v, &mod_info).await;

                        // Actualizar manifest
                        if let Some(meta) = &mod_info.metadata {
                            let mut manifest = crate::game::mods::load_manifest(&bd, &ch, &v).await;
                            if let Some(entry) =
                                manifest.iter_mut().find(|m| m.mod_id == meta.mod_id)
                            {
                                entry.enabled = !mod_info.enabled;
                                let _ =
                                    crate::game::mods::save_manifest(&bd, &ch, &v, &manifest).await;
                            }
                        }

                        Ok::<(), String>(())
                    },
                    |_| ModsMessage::RefreshLocalBackground,
                )
            }
            ModsMessage::DeleteLocal(mod_info) => {
                self.loading = true;
                let bd = base_dir.clone();
                let s = settings.clone();
                Task::perform(
                    async move {
                        // Eliminar el archivo físico
                        let _ = crate::game::mods::delete_mod(&mod_info).await;

                        // También eliminar del manifest si tiene metadatos
                        if let Some(meta) = &mod_info.metadata {
                            let version_str = if s.game_version == 0 {
                                "latest".to_string()
                            } else {
                                s.game_version.to_string()
                            };

                            let mut manifest =
                                crate::game::mods::load_manifest(&bd, &s.channel, &version_str)
                                    .await;
                            // Remover la entrada del mod eliminado
                            manifest.retain(|m| m.mod_id != meta.mod_id);
                            // Guardar el manifest actualizado
                            let _ = crate::game::mods::save_manifest(
                                &bd,
                                &s.channel,
                                &version_str,
                                &manifest,
                            )
                            .await;
                        }

                        Ok::<(), String>(())
                    },
                    |_| ModsMessage::RefreshLocalBackground,
                )
            }
            ModsMessage::OpenFolder => {
                let p = crate::game::GamePaths::new(base_dir);
                let v = if settings.game_version == 0 {
                    "latest"
                } else {
                    &settings.game_version.to_string()
                };
                crate::util::open_path(p.mods_dir(&settings.channel, v));
                Task::none()
            }
            ModsMessage::OpenPatchFolder => {
                let p = crate::game::GamePaths::new(base_dir);
                let v = if settings.game_version == 0 {
                    "latest"
                } else {
                    &settings.game_version.to_string()
                };
                crate::util::open_path(p.core_patches_dir(&settings.channel, v));
                Task::none()
            }
            ModsMessage::OpenJarFolder => {
                let p = crate::game::GamePaths::new(base_dir);
                let v = if settings.game_version == 0 {
                    "latest"
                } else {
                    &settings.game_version.to_string()
                };
                crate::util::open_path(p.mods_dir(&settings.channel, v));
                Task::none()
            }
            ModsMessage::SwitchTab(tab) => {
                self.current_tab = tab;
                if self.current_tab == ModTab::Browse && self.remote_mods.is_empty() {
                    return self.perform_search(client);
                }
                Task::none()
            }
            ModsMessage::SearchChanged(q) => {
                self.search_query = q;
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
                        let mut tasks = Vec::new();
                        for m in &self.remote_mods {
                            if let Some(logo) = &m.logo_url {
                                if !self.thumbnails.contains_key(&m.id) {
                                    let url = logo.clone();
                                    let id = m.id.clone();
                                    let c = client.clone();
                                    tasks.push(Task::perform(
                                        async move {
                                            (
                                                id,
                                                crate::util::image_cache::load_image(&c, &url)
                                                    .await
                                                    .map_err(|e| e.to_string()),
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
                    self.thumbnails.insert(id, h);
                }
                Task::none()
            }
            ModsMessage::VersionSelected { mod_id, file_id } => {
                // Para la pestaña Browse
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
                    if let Some(url) = &file.download_url {
                        let (uc, fnm, bdc) = (url.clone(), file.name.clone(), base_dir.clone());
                        let id_task = id_clone.clone();

                        return Task::perform(
                            async move {
                                let (c, v) = (
                                    settings.channel.clone(),
                                    if settings.game_version == 0 {
                                        "latest".to_string()
                                    } else {
                                        settings.game_version.to_string()
                                    },
                                );
                                let (md, _) =
                                    crate::game::mods::ensure_mod_dirs(&bdc, &c, &v).await;
                                let dest = md.join(&fnm);
                                match crate::game::downloader::download_file(
                                    &client,
                                    &uc,
                                    &dest,
                                    |_, _| {},
                                    None,
                                )
                                .await
                                {
                                    Ok(_) => {
                                        if let Ok(m) = tokio::fs::metadata(&dest).await {
                                            if m.len() == 0 {
                                                let _ = tokio::fs::remove_file(&dest).await;
                                                return Err("Empty file".to_string());
                                            }
                                        }
                                        Ok(fnm)
                                    }
                                    Err(e) => Err(e.to_string()),
                                }
                            },
                            move |res| ModsMessage::ModInstalled(res, id_task),
                        );
                    }
                }
                self.installing_ids.remove(&id_clone);
                self.error = Some("Selected version invalid or has no download URL".to_string());
                Task::none()
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
                        // Guardar metadatos si tenemos información del mod
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
                        if fnm.ends_with(".zip") && crate::game::zip_mods::is_patch_mod(&fp) {
                            Task::done(ModsMessage::InstallZipPatch(fp, settings, meta))
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
            ModsMessage::InstallZipPatch(zp, s, meta) => {
                self.loading = true;
                let bd = base_dir.clone();
                Task::perform(
                    async move {
                        let p = crate::game::GamePaths::new(bd);
                        let (c, v) = (
                            s.channel.clone(),
                            if s.game_version == 0 {
                                "latest".to_string()
                            } else {
                                s.game_version.to_string()
                            },
                        );
                        let (gd, pd) = (p.version_dir(&c, &v), p.core_patches_dir(&c, &v));
                        let nm = zp
                            .file_stem()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string();
                        let zpc = zp.clone();
                        tokio::task::spawn_blocking(move || {
                            crate::game::zip_mods::install_new_patch(
                                zp,
                                gd,
                                pd,
                                nm,
                                meta.as_ref().map(|m| m.mod_id.clone()),
                                meta.as_ref().map(|m| m.file_id.clone()),
                                meta.as_ref().map(|m| m.provider),
                                meta.as_ref().map(|m| m.summary.clone()).flatten(),
                                meta.as_ref().map(|m| m.logo_url.clone()).flatten(),
                            )
                        })
                        .await
                        .map_err(|e| e.to_string())?
                        .map_err(|e| e.to_string())?;
                        let _ = tokio::fs::remove_file(zpc).await;
                        Ok(())
                    },
                    ModsMessage::PatchOperationFinished,
                )
            }
            ModsMessage::ToggleZipPatch(id, en) => {
                self.loading = true;
                let bd = base_dir.clone();
                let s = settings.clone();
                Task::perform(
                    async move {
                        let p = crate::game::GamePaths::new(bd);
                        let (c, v) = (
                            s.channel,
                            if s.game_version == 0 {
                                "latest".to_string()
                            } else {
                                s.game_version.to_string()
                            },
                        );
                        let (gd, pd) = (p.version_dir(&c, &v), p.core_patches_dir(&c, &v));
                        tokio::task::spawn_blocking(move || {
                            if en {
                                crate::game::zip_mods::enable_patch(gd, pd, &id)
                            } else {
                                crate::game::zip_mods::disable_patch(gd, pd, &id)
                            }
                        })
                        .await
                        .unwrap()
                        .map_err(|e| e.to_string())
                    },
                    ModsMessage::PatchOperationFinished,
                )
            }
            ModsMessage::UninstallZipPatch(id, s) => {
                self.loading = true;
                let bd = base_dir.clone();
                Task::perform(
                    async move {
                        let p = crate::game::GamePaths::new(bd);
                        let (c, v) = (
                            s.channel.clone(),
                            if s.game_version == 0 {
                                "latest".to_string()
                            } else {
                                s.game_version.to_string()
                            },
                        );
                        let (gd, pd) = (p.version_dir(&c, &v), p.core_patches_dir(&c, &v));
                        tokio::task::spawn_blocking(move || {
                            crate::game::zip_mods::uninstall_patch(gd, pd, &id)
                        })
                        .await
                        .map_err(|e| e.to_string())?
                        .map_err(|e| e.to_string())?;
                        Ok(())
                    },
                    ModsMessage::PatchOperationFinished,
                )
            }
            ModsMessage::PatchOperationFinished(res) => {
                self.loading = false;
                match res {
                    Ok(_) => Task::done(ModsMessage::RefreshLocalBackground),
                    Err(e) => {
                        self.error = Some(format!("Failed: {}", e));
                        Task::none()
                    }
                }
            }
            ModsMessage::CheckForUpdates => {
                self.checking_updates = true;
                self.mods_with_updates.clear();

                let base_dir_clone = base_dir.clone();
                let settings_clone = settings.clone();

                Task::perform(
                    async move {
                        let mut updates = Vec::new();
                        let mut cached_map = std::collections::HashMap::new();

                        // Determinar version del juego "limpia" para comparar
                        let version_str = if settings_clone.game_version == 0 {
                            "latest".to_string()
                        } else {
                            settings_clone.game_version.to_string()
                        };

                        // Cargar mods instalados
                        let manifest = crate::game::mods::load_manifest(
                            &base_dir_clone,
                            &settings_clone.channel,
                            &version_str,
                        )
                        .await;

                        let repo = CurseForgeRepository::new();
                        let current_game_ver = version_str.clone();

                        // 1. Verificar JAR Mods
                        for installed in manifest {
                            if installed.provider == ModProvider::CurseForge {
                                if let Ok(versions) = repo.get_versions(&installed.mod_id).await {
                                    let compatible_file = versions.iter().find(|f| {
                                        current_game_ver == "latest"
                                            || f.game_versions.contains(&current_game_ver)
                                    });

                                    if let Some(latest) = compatible_file {
                                        if latest.file_id != installed.file_id {
                                            updates.push(installed.file_name.clone());
                                        }
                                        cached_map.insert(installed.mod_id.clone(), versions);
                                    }
                                }
                            }
                        }

                        // 2. Verificar ZIP Patches
                        let paths = crate::game::GamePaths::new(base_dir_clone.clone());
                        let patches = crate::game::zip_mods::list_patches(
                            paths.core_patches_dir(&settings_clone.channel, &version_str),
                        )
                        .unwrap_or_default();

                        for p in patches {
                            if let (Some(rid), Some(prov)) = (p.remote_id, p.provider) {
                                if prov == ModProvider::CurseForge {
                                    if let Ok(versions) = repo.get_versions(&rid).await {
                                        let compatible_file = versions.iter().find(|f| {
                                            current_game_ver == "latest"
                                                || f.game_versions.contains(&current_game_ver)
                                        });

                                        if let Some(latest) = compatible_file {
                                            // En patches, file_id es Option<String>
                                            if Some(latest.file_id.clone()) != p.file_id {
                                                // Usamos mod_id local del patch para marcarlo con update
                                                updates.push(p.mod_id.clone());
                                            }
                                            cached_map.insert(rid.clone(), versions);
                                        }
                                    }
                                }
                            }
                        }

                        Ok((updates, cached_map))
                    },
                    ModsMessage::UpdatesChecked,
                )
            }
            ModsMessage::UpdatesChecked(res) => {
                self.checking_updates = false;
                match res {
                    Ok((updates, versions)) => {
                        self.mods_with_updates = updates.into_iter().collect();
                        // Poblar el cache con lo que encontramos al buscar actualizaciones
                        for (mod_id, files) in versions {
                            self.cached_versions.insert(mod_id, files);
                        }
                    }
                    Err(e) => {
                        self.error = Some(format!("Error checking updates: {}", e));
                    }
                }
                Task::none()
            }
            // Método antiguo (Actualizar a la última)
            ModsMessage::UpdateMod(mod_info) => {
                // Redirige a UpdateModToVersion pasando "latest" logic implicitamente
                // O mejor, implementamos una llamada a UpdateModToVersion con None file_id logic interna
                // Para simplificar, este botón busca la última versión compatible automáticamente.
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
            // NUEVO: Método para cambiar a una versión específica (Upgrade/Downgrade)
            ModsMessage::UpdateModToVersion(mod_info, target_file_id, is_patch) => {
                if let Some(meta) = &mod_info.metadata {
                    let mod_id = meta.mod_id.clone();
                    let old_file_name = meta.file_name.clone();
                    let file_id_request = target_file_id.clone();

                    let client_clone = client.clone();
                    let base_dir_clone = base_dir.clone();
                    let settings_clone = settings.clone();
                    let mod_info_clone = mod_info.clone();

                    self.loading = true;
                    if let Some(meta) = &mod_info.metadata {
                        self.installing_ids.insert(meta.mod_id.clone());
                    }

                    Task::perform(
                        async move {
                            let repo = CurseForgeRepository::new();

                            // 1. Obtener todas las versiones (necesario para encontrar la URL del target_file_id)
                            let versions = repo
                                .get_versions(&mod_id)
                                .await
                                .map_err(|e| format!("API Error: {}", e))?;

                            // 2. Encontrar el archivo objetivo
                            let target_file = if file_id_request.is_empty() {
                                // Si viene vacío (botón Update simple), buscar el último compatible
                                let current_game_ver = if settings_clone.game_version == 0 {
                                    "latest".to_string()
                                } else {
                                    settings_clone.game_version.to_string()
                                };

                                versions
                                    .iter()
                                    .find(|f| {
                                        current_game_ver == "latest"
                                            || f.game_versions.contains(&current_game_ver)
                                    })
                                    .or_else(|| versions.first())
                                    .ok_or("No compatible version found")?
                            } else {
                                // Buscar el ID específico
                                versions
                                    .iter()
                                    .find(|f| f.file_id == file_id_request)
                                    .ok_or("Selected version not found in metadata")?
                            };

                            let download_url = target_file
                                .download_url
                                .clone()
                                .ok_or("No download URL for version")?;

                            let new_file_name = target_file.name.clone();

                            // 3. Rutas
                            let version_str = if settings_clone.game_version == 0 {
                                "latest".to_string()
                            } else {
                                settings_clone.game_version.to_string()
                            };
                            let (mods_dir, disabled_dir) = crate::game::mods::ensure_mod_dirs(
                                &base_dir_clone,
                                &settings_clone.channel,
                                &version_str,
                            )
                            .await;

                            let old_path = mods_dir.join(&old_file_name);
                            let new_path = mods_dir.join(&new_file_name);

                            if is_patch {
                                // 1. Desinstalar parche viejo (usando old_file_name que es el ID del patch local)
                                let p = crate::game::GamePaths::new(base_dir_clone.clone());
                                let (c, v) = (
                                    settings_clone.channel.clone(),
                                    if settings_clone.game_version == 0 {
                                        "latest".to_string()
                                    } else {
                                        settings_clone.game_version.to_string()
                                    },
                                );
                                let (gd, pd) = (p.version_dir(&c, &v), p.core_patches_dir(&c, &v));
                                let old_id = old_file_name.clone();
                                let _ = tokio::task::spawn_blocking(move || {
                                    crate::game::zip_mods::uninstall_patch(gd, pd, &old_id)
                                })
                                .await;
                            }

                            // 4. Descargar
                            crate::game::downloader::download_file(
                                &client_clone,
                                &download_url,
                                &new_path,
                                |_, _| {},
                                None,
                            )
                            .await
                            .map_err(|e| e.to_string())?;

                            // 5. Borrar el viejo (manejar tanto activos como desactivados)
                            // Primero el desactivado (siempre, para evitar que quede huérfano)
                            let disabled_old_path = disabled_dir.join(&old_file_name);
                            if disabled_old_path.exists() {
                                let _ = tokio::fs::remove_file(&disabled_old_path).await;
                            }

                            // Luego el activo solo si el nombre cambió (si es igual, download_file ya lo pisó)
                            if old_file_name != new_file_name {
                                if old_path.exists() {
                                    let _ = tokio::fs::remove_file(&old_path).await;
                                }
                            }

                            // 6. Actualizar el Manifiesto JSON
                            let mut manifest = crate::game::mods::load_manifest(
                                &base_dir_clone,
                                &settings_clone.channel,
                                &version_str,
                            )
                            .await;

                            if let Some(entry) = manifest.iter_mut().find(|m| m.mod_id == mod_id) {
                                entry.file_name = new_file_name.clone();
                                entry.file_id = target_file.file_id.clone();
                                entry.enabled = true; // Forzar habilitado al actualizar
                                entry.install_date = chrono::Utc::now();
                                entry.update_available = None;
                            }

                            crate::game::mods::save_manifest(
                                &base_dir_clone,
                                &settings_clone.channel,
                                &version_str,
                                &manifest,
                            )
                            .await
                            .ok();

                            Ok((
                                new_file_name,
                                mod_info_clone,
                                target_file.file_id.clone(),
                                is_patch,
                            ))
                        },
                        ModsMessage::UpdateModDownloaded,
                    )
                } else {
                    Task::none()
                }
            }
            ModsMessage::UpdateModDownloaded(res) => {
                self.loading = false;
                match res {
                    Ok((new_file_name, mod_info, file_id, is_patch)) => {
                        // Limpiar estado de instalacion
                        if let Some(meta) = &mod_info.metadata {
                            self.installing_ids.remove(&meta.mod_id);
                        }

                        if is_patch {
                            // Si era un parche, ahora que bajamos el nuevo ZIP, hay que instalarlo.
                            // Construimos la ruta donde se bajó (mods_dir temporal)
                            let (bdc, sc, sv) = (
                                base_dir.clone(),
                                settings.channel.clone(),
                                if settings.game_version == 0 {
                                    "latest".to_string()
                                } else {
                                    settings.game_version.to_string()
                                },
                            );
                            let paths = crate::game::GamePaths::new(bdc.clone());
                            let mods_dir = paths.mods_dir(&sc, &sv);
                            let fp = mods_dir.join(&new_file_name);

                            // El ModInfo + file_id nuevo se convierte en metadata
                            let meta = if let Some(old_meta) = mod_info.metadata {
                                Some(InstalledModMetadata {
                                    file_name: new_file_name.clone(),
                                    mod_name: old_meta.mod_name,
                                    provider: old_meta.provider,
                                    mod_id: old_meta.mod_id,
                                    file_id,
                                    enabled: true, // Siempre habilitar al actualizar para asegurar consistencia
                                    summary: old_meta.summary.clone(),
                                    logo_url: old_meta.logo_url.clone(),
                                    install_date: chrono::Utc::now(),
                                    update_available: None,
                                })
                            } else {
                                None
                            };

                            Task::done(ModsMessage::InstallZipPatch(fp, settings, meta))
                        } else {
                            Task::done(ModsMessage::RefreshLocalBackground)
                        }
                    }
                    Err(e) => {
                        self.error = Some(format!("Update failed: {}", e));
                        Task::none()
                    }
                }
            }
            ModsMessage::UpdateModStart(_) => Task::none(),
            ModsMessage::LoadVersions(mod_id) => {
                if self.loading_versions.contains(&mod_id)
                    || self.cached_versions.contains_key(&mod_id)
                {
                    return Task::none();
                }

                self.loading_versions.insert(mod_id.clone());

                Task::perform(
                    async move {
                        let repo = CurseForgeRepository::new();
                        match repo.get_versions(&mod_id).await {
                            Ok(versions) => Ok((mod_id, versions)),
                            Err(e) => Err(format!("Failed to load versions: {}", e)),
                        }
                    },
                    ModsMessage::VersionsLoaded,
                )
            }
            ModsMessage::VersionsLoaded(res) => {
                match res {
                    Ok((mod_id, versions)) => {
                        // Quitamos del set de carga
                        self.loading_versions.remove(&mod_id);
                        // ¡IMPORTANTE! Guardamos en caché
                        self.cached_versions.insert(mod_id, versions);
                    }
                    Err(e) => {
                        // Si falla, limpiar para permitir reintento
                        self.loading_versions.clear();
                        self.error = Some(format!("Version fetch failed: {}", e));
                    }
                }
                Task::none()
            }
        }
    }

    fn load_mods_task(&self, bd: std::path::PathBuf, s: GameSettings) -> Task<ModsMessage> {
        let (bdc, c, v) = (
            bd.clone(),
            s.channel.clone(),
            if s.game_version == 0 {
                "latest".to_string()
            } else {
                s.game_version.to_string()
            },
        );
        Task::perform(
            async move {
                let j = crate::game::mods::list_mods(&bdc, &c, &v)
                    .await
                    .map_err(|e| e.to_string())?;
                let p = crate::game::zip_mods::list_patches(
                    crate::game::GamePaths::new(bdc).core_patches_dir(&c, &v),
                )
                .map_err(|e| e.to_string())?;
                Ok((j, p))
            },
            |res| ModsMessage::ModsLoadedComplex(res),
        )
    }

    fn perform_search(&mut self, _cl: reqwest::Client) -> Task<ModsMessage> {
        self.loading = true;
        let (q, i, l) = (
            self.search_query.clone(),
            self.current_page * self.page_size,
            self.page_size,
        );
        Task::perform(
            async move {
                // Use generic repository interface
                let repo = CurseForgeRepository::new();
                repo.search(&q, i, l).await.map_err(|e| e.to_string())
            },
            ModsMessage::SearchLoaded,
        )
    }

    pub fn view<'a>(
        &'a self,
        localization: &'a crate::lang::Localization,
        ws: Size,
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
            ModTab::Installed => self.view_installed(localization, ctx),
            ModTab::Browse => self.view_browse(localization, is_c, ctx),
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
        localization: &'a crate::lang::Localization,
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
                                svg(util::icons::icon(util::icons::FOLDER))
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
                let has_update = self.mods_with_updates.contains(&p.mod_id);
                pl = pl.push(self.view_patch_row(p, has_update, localization, ctx));
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
                            svg(util::icons::icon(util::icons::FOLDER))
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
            for m in &self.installed_mods {
                let has_update = self.mods_with_updates.contains(&m.file_name);
                // Pasamos `self` completo o las referencias necesarias para la lógica del dropdown
                ml = ml.push(self.view_installed_row(m, has_update, localization, ctx));
            }
        } else if self.patch_mods.is_empty() {
            ml = ml.push(theme::text_caption(localization.t("mods.no_mods_jar"), ctx));
        }
        content_items.push(theme::magic_container(ml.into(), ctx));

        theme::magic_scrollable(
            scrollable(theme::magic_column(content_items, ctx))
                .id(Id::new(MODS_SCROLL_ID))
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
        localization: &'a crate::lang::Localization,
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

            // Check si las versiones están cargadas en memoria o cargando
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
                // NO TENEMOS VERSIONES: Botón Lazy "Check Versions"
                let load_btn = button(
                    row![
                        theme::svg(
                            svg(util::icons::icon(util::icons::REFRESH))
                                .width(10)
                                .height(10)
                                .style(move |_, _| svg::Style {
                                    color: Some(palette.text_secondary)
                                }),
                            ctx
                        ),
                        theme::text_micro("Load versions", ctx)
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
            center_info = center_info.push(theme::text_caption("Manual Install", ctx));
        }

        // --------------------------------------------------------
        // ACCIONES DERECHA
        // --------------------------------------------------------

        if is_installing {
            // Mientras instala no permitimos acciones
            actions.push(theme::text_micro("WAIT...", ctx).into());
        } else {
            // Botón UPDATE funcional cuando hay actualización disponible
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
                    svg(util::icons::icon(util::icons::TRASH))
                        .width(14)
                        .height(14)
                        .style(|_, _| iced::widget::svg::Style {
                            color: Some(Color::BLACK),
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
                        if let Some(h) = self.thumbnails.get(&meta.mod_id) {
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
                                svg(util::icons::icon(util::icons::PUZZLE))
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
                            svg(util::icons::icon(util::icons::PUZZLE))
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
                    // Estado visual pequeño (opcional)
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
        localization: &'a crate::lang::Localization,
        is_c: bool,
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
            let l = column(
                self.remote_mods
                    .iter()
                    .map(|m| self.view_remote_card(m, localization, ctx))
                    .collect::<Vec<_>>(),
            )
            .spacing(10);
            theme::magic_container(
                container(theme::magic_scrollable(
                    scrollable(l)
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
        localization: &'a crate::lang::Localization,
        ctx: theme::UIContext,
    ) -> Element<'a, ModsMessage, Theme, Renderer> {
        let (isi, isin) = (
            self.installing_ids.contains(&cf.id),
            self.installed_ids.contains(&cf.id),
        );

        // 1. Obtener archivos disponibles para este mod
        // Usar cache si existe, sino los archivos iniciales de la búsqueda
        let files = if let Some(cached) = self.cached_versions.get(&cf.id) {
            cached
        } else {
            &cf.latest_files
        };

        // 2. Verificar si está cargando versiones
        let is_loading = self.loading_versions.contains(&cf.id);

        // 3. Buscar cuál está seleccionado actualmente (o default) - PATRÓN DE SETTINGS
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
            // Default inteligente: El más nuevo compatible
            files
                .iter()
                .find(|f| {
                    current_game_ver == "latest" || f.game_versions.contains(&current_game_ver)
                })
                .or(files.first())
        };

        // 4. Crear el Dropdown (PickList) con botón de carga adicional
        let is_full_list_loaded = self.cached_versions.contains_key(&cf.id);
        let needs_load = !is_full_list_loaded && !is_loading;

        let version_selector: Element<'a, ModsMessage, Theme, Renderer> = if is_loading {
            // Mostrar "Fetching versions..." mientras carga
            row![theme::text_caption("Fetching versions...", ctx),].into()
        } else if files.is_empty() {
            // Fallback visual si la API no trajo archivos
            theme::text_caption("No versions found", ctx).into()
        } else {
            let pick = pick_list(
                files.as_slice(), // GenericFile implementa Display y PartialEq
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

            let dropdown = theme::magic_pick_list_with_menu(pick.into(), ctx);

            // Si necesita cargar más versiones, mostrar botón de refresh al lado
            if needs_load {
                row![
                    container(dropdown).width(Length::Fill),
                    theme::magic_button(
                        button(theme::svg(
                            svg(util::icons::icon(util::icons::REFRESH))
                                .width(12)
                                .height(12)
                                .style(move |_, _| svg::Style {
                                    color: Some(ctx.palette.text_secondary)
                                }),
                            ctx
                        ))
                        .on_press(ModsMessage::LoadVersions(cf.id.clone()))
                        .style(move |t, s| theme::ghost_button_style(&ctx.palette, t, s))
                        .padding(8)
                        .into(),
                        ctx
                    )
                ]
                .spacing(5)
                .width(Length::Fixed(240.0))
                .into()
            } else {
                // Si ya tenemos todo, solo el dropdown
                container(dropdown).width(Length::Fixed(240.0)).into()
            }
        };

        // 4. Modificar el Botón de Instalar para usar la selección
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
            if let Some(h) = self.thumbnails.get(&cf.id) {
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
                    // Añadimos selector aquí debajo del título
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

        // Hacer la card cliqueable para abrir la página del mod
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
        localization: &'a crate::lang::Localization,
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
                            svg(util::icons::icon(util::icons::REFRESH))
                                .width(10)
                                .height(10)
                                .style(move |_, _| svg::Style {
                                    color: Some(palette.text_secondary)
                                }),
                            ctx
                        ),
                        theme::text_micro("Load versions", ctx)
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
            center_info = center_info.push(theme::text_caption("Manual Patch", ctx));
        }

        // ACCIONES
        if is_installing {
            actions.push(theme::text_micro("WAIT...", ctx).into());
        } else {
            // Botón UPDATE funcional cuando hay actualización disponible
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
                    svg(util::icons::icon(util::icons::TRASH))
                        .width(14)
                        .height(14)
                        .style(|_, _| iced::widget::svg::Style {
                            color: Some(Color::BLACK),
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
                    Element::from(if let Some(logo) = &p.logo_url {
                        if let Some(rid) = &p.remote_id {
                            if let Some(h) = self.thumbnails.get(rid) {
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
                                    svg(util::icons::icon(util::icons::ZIP))
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
                                svg(util::icons::icon(util::icons::ZIP))
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
                            svg(util::icons::icon(util::icons::ZIP))
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

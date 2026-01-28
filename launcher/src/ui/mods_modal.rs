use crate::config::GameSettings;
use crate::game::curseforge::{CfMod, SearchResult};
use crate::game::mods::ModInfo;
use crate::game::zip_mods::PatchManifest;
use crate::{theme, util};
use iced::widget::{
    Space, button, column, container, image, row, scrollable, svg, text_input,
};
use iced::{Alignment, Color, ContentFit, Element, Length, Renderer, Size, Task, Theme};
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
    SearchLoaded(Result<SearchResult, String>),
    ImageLoaded(i32, Result<image::Handle, String>),
    InstallMod(CfMod),
    ModInstalled(Result<String, String>, i32),
    OpenModPage(String),
    InstallZipPatch(std::path::PathBuf, GameSettings),
    ToggleZipPatch(String, bool),
    UninstallZipPatch(String, GameSettings),
    PatchOperationFinished(Result<(), String>),
    ModsLoadedComplex(Result<(Vec<ModInfo>, Vec<crate::game::zip_mods::PatchManifest>), String>),
    OpenMods,
}

pub struct ModsState {
    pub is_open: bool,
    pub current_tab: ModTab,
    pub search_query: String,
    pub remote_mods: Vec<CfMod>,
    pub current_page: u32,
    pub total_results: u32,
    pub page_size: u32,
    pub thumbnails: HashMap<i32, image::Handle>,
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
                            if let Some(logo) = &m.logo {
                                if !self.thumbnails.contains_key(&m.id) {
                                    let url = logo.thumbnail_url.clone();
                                    let id = m.id;
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
            ModsMessage::InstallMod(cf_mod) => {
                self.installing_ids.insert(cf_mod.id);
                let id = cf_mod.id;
                if let Some(file) = cf_mod.latest_files.first() {
                    if let Some(url) = &file.download_url {
                        let (uc, fnm, bdc) =
                            (url.clone(), file.file_name.clone(), base_dir.clone());
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
                                                return Err("Empty".to_string());
                                            }
                                        }
                                        Ok(fnm)
                                    }
                                    Err(e) => Err(e.to_string()),
                                }
                            },
                            move |res| ModsMessage::ModInstalled(res, id),
                        );
                    }
                }
                self.installing_ids.remove(&id);
                self.error = Some("No URL".to_string());
                Task::none()
            }
            ModsMessage::OpenModPage(url) => {
                let _ = open::that(url);
                Task::none()
            }
            ModsMessage::ModInstalled(res, id) => {
                self.installing_ids.remove(&id);
                match res {
                    Ok(fnm) => {
                        self.installed_ids.insert(id);
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
                            Task::done(ModsMessage::InstallZipPatch(fp, settings))
                        } else {
                            Task::done(ModsMessage::RefreshLocalBackground)
                        }
                    }
                    Err(e) => {
                        self.error = Some(format!("Failed: {}", e));
                        Task::none()
                    }
                }
            }
            ModsMessage::InstallZipPatch(zp, s) => {
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
                            crate::game::zip_mods::install_new_patch(zp, gd, pd, nm)
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

    fn perform_search(&mut self, cl: reqwest::Client) -> Task<ModsMessage> {
        self.loading = true;
        let (q, i, l) = (
            self.search_query.clone(),
            self.current_page * self.page_size,
            self.page_size,
        );
        Task::perform(
            async move {
                crate::game::curseforge::search_mods(&cl, &q, i, l)
                    .await
                    .map_err(|e| e.to_string())
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
                tab_btn(
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
                tab_btn(
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
                pl = pl.push(patch_row(p, &self.temp_settings, localization, ctx));
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
            )
        ]
        .align_y(Alignment::Center);

        let mut ml = column![Element::from(hj)].spacing(10);
        if !self.installed_mods.is_empty() {
            for m in &self.installed_mods {
                ml = ml.push(mod_row(m, localization, ctx));
            }
        } else if self.patch_mods.is_empty() {
            ml = ml.push(theme::text_caption(
                localization.t("mods.no_mods_jar"),
                ctx,
            ));
        }
        content_items.push(theme::magic_container(ml.into(), ctx));

        theme::magic_scrollable(
            scrollable(theme::magic_column(content_items, ctx))
                .height(Length::Fill)
                .style(move |t: &Theme, s| theme::scrollable_style(&palette, t, s))
                .into(),
            ctx,
        )
        .into()
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
        cf: &'a CfMod,
        localization: &'a crate::lang::Localization,
        ctx: theme::UIContext,
    ) -> Element<'a, ModsMessage, Theme, Renderer> {
        let (isi, isin) = (
            self.installing_ids.contains(&cf.id),
            self.installed_ids.contains(&cf.id),
        );

        let ab = if isi {
            button(theme::text_caption(
                localization.t("launcher.status.downloading"),
                ctx,
            ))
            .style(move |t: &Theme, s| theme::ghost_button_style(&ctx.palette, t, s))
        } else if isin {
            button(theme::text_caption(localization.t("mods.installed"), ctx))
                .style(move |t: &Theme, s| theme::success_button_style(&ctx.palette, t, s))
        } else {
            button(theme::text_caption(localization.t("mods.install"), ctx))
                .on_press(ModsMessage::InstallMod(cf.clone()))
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

        theme::magic_container(
            theme::list_item_row(
                row![
                    th,
                    column![
                        theme::text_body(&cf.name, ctx),
                        theme::text_caption(&cf.summary, ctx),
                        theme::text_micro(format!(
                            "{}: {:.0}",
                            localization.t("mods.downloads"),
                            cf.download_count
                        ), ctx)
                    ]
                    .spacing(4)
                    .width(Length::Fill)
                ]
                .spacing(15)
                .align_y(Alignment::Center)
                .into(),
                vec![theme::magic_button(ab.into(), ctx)],
                ctx,
            )
            .into(),
            ctx,
        )
    }
}

pub fn mod_row<'a>(
    mi: &'a ModInfo,
    localization: &'a crate::lang::Localization,
    ctx: theme::UIContext,
) -> Element<'a, ModsMessage, Theme, Renderer> {
    let palette = ctx.palette;

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

    theme::magic_container(
        theme::list_item_row(
            row![
                theme::text_body(&mi.name, ctx),
                Space::new().width(10),
                theme::text_small(if mi.enabled {
                    localization.t("mods.active")
                } else {
                    localization.t("mods.disabled")
                }, ctx),
            ]
            .align_y(Alignment::Center)
            .into(),
            vec![tb.into(), db.into()],
            ctx,
        )
        .into(),
        ctx,
    )
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

fn patch_row<'a>(
    p: &'a PatchManifest,
    _curr: &'a GameSettings,
    localization: &'a crate::lang::Localization,
    ctx: theme::UIContext,
) -> Element<'a, ModsMessage, Theme, Renderer> {
    let palette = ctx.palette;

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
            _curr.clone(),
        ))
        .style(move |t: &Theme, s| theme::danger_button_style(&palette, t, s))
        .padding(5)
        .into(),
        ctx,
    );

    theme::magic_container(
        theme::list_item_row(
            row![
                theme::svg(
                    svg(util::icons::icon(util::icons::ZIP))
                        .width(20)
                        .height(20)
                        .style(|_, _| iced::widget::svg::Style {
                            color: Some(Color::BLACK),
                        }),
                    ctx,
                ),
                column![
                    theme::text_body(&p.mod_name, ctx),
                    row![
                        theme::text_micro(if p.enabled {
                            localization.t("mods.active")
                        } else {
                            localization.t("mods.disabled")
                        }, ctx),
                        theme::text_body("|", ctx),
                        theme::text_caption(&p.install_date.format("%Y-%m-%d").to_string(), ctx),
                    ]
                    .spacing(5)
                ]
            ]
            .spacing(10)
            .align_y(Alignment::Center)
            .into(),
            vec![tb.into(), db.into()],
            ctx,
        )
        .into(),
        ctx,
    )
}

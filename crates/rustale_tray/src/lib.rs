use tray_icon::{
    TrayIcon, TrayIconBuilder,
    menu::{Menu, MenuItem, MenuEvent},
    TrayIconEvent,
};
use image::imageops::FilterType;

pub use tray_icon;

pub fn load_tray_icon(icon_bytes: &[u8]) -> tray_icon::Icon {
    let image = image::load_from_memory(icon_bytes).expect("Failed to load tray icon");

    // Redimensionamos a 32x32 para asegurar compatibilidad (especialmente en Linux/KDE)
    let resized = image.resize_exact(32, 32, FilterType::Lanczos3);
    let rgba_image = resized.to_rgba8();

    let width = rgba_image.width();
    let height = rgba_image.height();
    let rgba = rgba_image.into_raw();

    tray_icon::Icon::from_rgba(rgba, width, height).expect("Failed to build tray icon")
}

pub struct TrayManager {
    pub tray_icon: Option<TrayIcon>,
}

impl TrayManager {
    pub fn new() -> Self {
        Self { tray_icon: None }
    }

    pub fn create_tray(
        &mut self,
        is_playing: bool,
        icon: tray_icon::Icon,
        tooltip: &str,
        localization: &rustale_shared::lang::Localization,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let tray_menu = Menu::new();
        
        let game_action = if is_playing {
            MenuItem::with_id("stop", localization.t("tray.stop_game"), true, None)
        } else {
            MenuItem::with_id("start", localization.t("tray.start_game"), true, None)
        };
        
        let show_i = MenuItem::with_id("show_hide", localization.t("tray.show_hide"), true, None);
        let quit_i = MenuItem::with_id("quit", localization.t("tray.quit"), true, None);

        let _ = tray_menu.append_items(&[&game_action, &show_i, &quit_i])?;

        let tray_icon = TrayIconBuilder::new()
            .with_menu(Box::new(tray_menu))
            .with_tooltip(tooltip)
            .with_icon(icon)
            .build()?;

        self.tray_icon = Some(tray_icon);
        Ok(())
    }

    pub fn destroy(&mut self) {
        self.tray_icon = None;
    }
}

pub fn receive_tray_event() -> Option<TrayIconEvent> {
    TrayIconEvent::receiver().try_recv().ok()
}

pub fn receive_menu_event() -> Option<MenuEvent> {
    MenuEvent::receiver().try_recv().ok()
}

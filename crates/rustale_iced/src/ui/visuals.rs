use crate::theme;
use crate::ui::{background_blur, lsd_shader};
use crate::config::GameSettings;
use crate::messages::Message;
use iced::Size;
use std::time::Instant;

pub struct VisualState {
    pub window_size: Size,
    pub is_maximized: bool,
    pub is_minimized: bool,
    pub is_focused: bool,
    pub is_fullscreen: bool,
    pub is_cursor_hidden: bool,
    pub last_mouse_move_time: Instant,
    pub last_user_interaction: Instant,
    pub last_mouse_update_time: Instant,
    pub mouse_update_interval: std::time::Duration,
    pub is_mouse_pressed: bool,
    pub last_mouse_release_time: Instant,
    pub lsd_offset: (f32, f32),
    pub start_time: Instant,
    pub current_time: f32,
    pub lsd_preview: bool,
    pub lsd_enabled_time: Option<Instant>,
    pub lsd_shader_instance: std::cell::RefCell<Option<lsd_shader::LsdShader>>,
    pub active_shader_idx: u32,
    pub next_shader_idx: u32,
    pub shader_transition: f32,
    pub total_shaders_available: u32,
    pub shader_change_timer: f32,
    pub ui_opacity_accumulator: f32,
    pub shader_click_intensity: f32,
    pub shader_click_time: Instant,
    pub background_blur: Option<background_blur::BackgroundBlur>,
    pub palette: theme::Palette,
    pub profile_dropdown_open: bool,
    pub editing_profile: Option<(Option<uuid::Uuid>, String)>, // (ID, Name)
    pub editing_uuid: Option<(uuid::Uuid, String)>,            // (ID, UUID_STRING)
    pub lsd_preview_override: Option<bool>, 
    pub is_visible: bool,
    #[cfg(all(feature = "tray", windows))]
    pub tray_manager: rustale_tray::TrayManager,
}

impl VisualState {
    #[cfg(all(feature = "tray", windows))]
    pub fn get_tray_manager(&self) -> &rustale_tray::TrayManager {
        &self.tray_manager
    }
    
    #[cfg(all(feature = "tray", windows))]
    pub fn get_tray_manager_mut(&mut self) -> &mut rustale_tray::TrayManager {
        &mut self.tray_manager
    }
    
    pub fn get_last_mouse_release_duration(&self) -> std::time::Duration {
        self.last_mouse_release_time.elapsed()
    }
}

impl VisualState {
    pub fn new(total_shaders: u32, palette: theme::Palette) -> Self {
        Self {
            window_size: Size::new(800.0, 600.0),
            is_maximized: false,
            is_minimized: false,
            is_focused: true,
            is_fullscreen: false,
            is_cursor_hidden: false,
            last_mouse_move_time: Instant::now(),
            last_user_interaction: Instant::now(),
            last_mouse_update_time: Instant::now(),
            mouse_update_interval: std::time::Duration::from_millis(16),
            is_mouse_pressed: false,
            last_mouse_release_time: Instant::now(),
            lsd_offset: (0.0, 0.0),
            start_time: Instant::now(),
            current_time: 0.0,
            lsd_preview: false,
            lsd_enabled_time: None,
            lsd_shader_instance: std::cell::RefCell::new(None),
            active_shader_idx: 0,
            next_shader_idx: 0,
            shader_transition: 0.0,
            total_shaders_available: total_shaders,
            shader_change_timer: 0.0,
            ui_opacity_accumulator: 1.0,
            shader_click_intensity: 0.0,
            shader_click_time: Instant::now(),
            background_blur: None,
            palette,
            profile_dropdown_open: false,
            editing_profile: None,
            editing_uuid: None,
            lsd_preview_override: None,
            is_visible: true,
            #[cfg(all(feature = "tray", windows))]
            tray_manager: rustale_tray::TrayManager::new(),
        }
    }

    /// Handle cursor movement with throttling and LSD mode restoration
    pub fn handle_cursor_moved(&mut self, _relative_position: iced::Point, is_modal_active: bool, settings: &GameSettings) {
        // [MOUSE THROTTLING] Solo actualizar si ha pasado suficiente tiempo desde la última actualización
        let now = std::time::Instant::now();
        let time_since_last_update = now.duration_since(self.last_mouse_update_time);
        
        if time_since_last_update >= self.mouse_update_interval {
            // Actualizar posición y tiempos
            self.last_mouse_move_time = now;
            self.last_user_interaction = now; // Reset interacción
            self.last_mouse_update_time = now; // Actualizar tiempo del último update
            
            // [GIRO PREDICTIVO] Registrar actividad del usuario
            crate::util::register_activity();
            
            let lsd_active = self.lsd_preview_override.unwrap_or(settings.theme.lsd_mode);
            if lsd_active && !is_modal_active {
                // Restaurar opacidad inmediatamente si está baja
                if self.ui_opacity_accumulator < 0.9 {
                    self.ui_opacity_accumulator = 1.0;
                    
                    // Mostrar cursor si estaba oculto
                    if self.is_cursor_hidden {
                        self.is_cursor_hidden = false;
                    }
                }
            }

            // Si el cursor estaba oculto y el usuario mueve el mouse, lo mostramos inmediatamente
            if self.is_cursor_hidden {
                self.is_cursor_hidden = false;
            }
        }
    }

    /// Handle tick updates for animations, shaders, and opacity management
    pub fn handle_tick(&mut self, is_modal_active: bool, settings: &GameSettings) {
        let frame_time = self.start_time.elapsed().as_secs_f32();
        self.current_time = frame_time;

        if self.is_focused && !self.is_minimized {
            crate::util::register_activity();
        }

        if let Some(ref mut lsd) = *self.lsd_shader_instance.borrow_mut() {
            lsd.update_time(frame_time);
        }
        
        // Determine if LSD logic should run
        let lsd_active = self.lsd_preview_override.unwrap_or(settings.theme.lsd_mode);

        // --- FIXED LOGIC: Opacity calculation ---
        let dt = 0.016; 
        let fade_speed = 1.5; 
        let reveal_speed = 2.5; 
        let hold_threshold = 0.25; 
        let inactivity_threshold = 10.0; 

        // Check conditions
        let elapsed_since_click = self.shader_click_time.elapsed().as_secs_f32();
        let elapsed_idle = self.last_mouse_move_time.elapsed().as_secs_f32();

        // Condition for hiding UI:
        // 1. Mouse is pressed AND held longer than threshold
        // 2. OR Inactivity timer exceeded
        // 3. AND NO modal is open
        let should_hide_ui = !is_modal_active && lsd_active && (
            (self.is_mouse_pressed && elapsed_since_click > hold_threshold) || 
            (elapsed_idle > inactivity_threshold)
        );

        if should_hide_ui {
            self.ui_opacity_accumulator = (self.ui_opacity_accumulator - dt * fade_speed).max(0.0);
            if self.ui_opacity_accumulator < 0.05 {
                self.is_cursor_hidden = true;
            }
        } else {
            self.ui_opacity_accumulator = (self.ui_opacity_accumulator + dt * reveal_speed).min(1.0);
            if self.ui_opacity_accumulator > 0.1 {
                self.is_cursor_hidden = false;
            }
        }

        // --- Shader Animation Logic ---
        let t = self.start_time.elapsed().as_secs_f32();
        // Slower, smoother movement
        let ox = (t * 0.5).sin() * 1.0 + (t * 1.2).cos() * 0.5; 
        let oy = (t * 0.4).cos() * 1.0 + (t * 1.5).sin() * 0.5;
        self.lsd_offset = (ox, oy);

        // --- Shader transition logic ---
        if self.shader_transition > 0.0 {
            self.shader_transition += 0.02;

            if self.shader_transition >= 1.0 {
                self.active_shader_idx = self.next_shader_idx;
                self.shader_transition = 0.0;
            }
        } else {
            self.shader_change_timer += dt; 
            if self.shader_change_timer > 30.0 {
                self.shader_change_timer = 0.0;
                self.next_shader_idx =
                    (self.active_shader_idx + 1) % self.total_shaders_available;
                self.shader_transition = 0.01; 
            }
        }
    }

    /// Process strictly visual messages (Tick, Mouse, Resize, Shader)
    /// Returns: Should the main loop request a redraw?
    pub fn process_message(&mut self, message: &Message, is_modal_active: bool, settings: &GameSettings) -> bool {
        match message {
            Message::CursorMoved(relative_position) => {
                // IMPORTANT: Directly update position here to ensure physics get latest data immediately
                self.handle_cursor_moved(*relative_position, is_modal_active, settings);
                true
            },
            Message::MousePressed => {
                self.is_mouse_pressed = true;
                // Reset click time for "Hold to hide" calculation
                self.shader_click_time = Instant::now(); 
                
                self.last_user_interaction = Instant::now();
                crate::util::register_activity();
                
                // Trigger visual pulse
                self.shader_click_intensity = 1.5;
                true
            },
            Message::MouseReleased => {
                self.is_mouse_pressed = false;
                self.last_mouse_release_time = Instant::now();
                // When released, UI should reappear immediately (handled by handle_tick)
                true
            },
            Message::Tick(_now) => {
                self.handle_tick(is_modal_active, settings);
                true
            },
            Message::WindowResized(size) => {
                self.window_size = *size;
                true
            },
            Message::WindowResizedWithMaximized(size, is_maximized) => {
                self.window_size = *size;
                self.is_maximized = *is_maximized;
                true
            },
            Message::NextShader => {
                self.next_shader_idx = (self.next_shader_idx + 1) % self.total_shaders_available;
                self.shader_click_time = std::time::Instant::now();
                self.shader_transition = 0.0;
                true
            },
            Message::NextShaderManual => {
                // Manual shader change without restrictions
                self.next_shader_idx =
                    (self.active_shader_idx + 1) % self.total_shaders_available;
                self.shader_transition = 0.01;
                self.shader_change_timer = 0.0;

                let new_shader_code =
                    crate::ui::shader_manager::build_uber_shader_with_index(
                        self.next_shader_idx as usize,
                    );
                crate::ui::lsd_shader::set_global_wgsl(new_shader_code);
                true
            },
            Message::WindowEvent(_id, event) => {
                match event {
                    iced::window::Event::Focused => {
                        self.is_focused = true;
                        true
                    },
                    iced::window::Event::Unfocused => {
                        self.is_focused = false;
                        true
                    },
                    _ => false,
                }
            },
            _ => false,
        }
    }
}

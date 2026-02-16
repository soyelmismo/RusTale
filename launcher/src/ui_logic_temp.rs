use crate::main::{Message, RusTale}; // This is pseudo-code context
use iced::{Task, window, Point, Size};
use std::sync::atomic::Ordering;

// This content will be injected into main.rs impl RusTale
impl RusTale {
    fn handle_ui_message(&mut self, message: &Message) -> Option<Task<Message>> {
        match message {
            Message::CursorMoved(relative_position) => {
                // [MOUSE THROTTLING] Solo actualizar si ha pasado suficiente tiempo desde la última actualización
                let now = std::time::Instant::now();
                let time_since_last_update = now.duration_since(self.last_mouse_update_time);
                
                if time_since_last_update >= self.mouse_update_interval {
                    // Actualizar posición y tiempos
                    self.cursor_position = *relative_position;
                    self.last_mouse_move_time = now;
                    self.last_user_interaction = now; // Reset interacción
                    self.last_mouse_update_time = now; // Actualizar tiempo del último update
                    
                    // [GIRO PREDICTIVO] Registrar actividad del usuario
                    crate::util::register_activity();
                    
                    // [FIX LSD] Restauración inmediata de opacidad al mover mouse
                    if self.settings.theme.lsd_mode && !self.settings_state.is_open && !self.mods_state.is_open {
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
                Some(Task::none())
            }

            Message::MousePressed => {
                self.is_mouse_pressed = true;
                self.last_user_interaction = std::time::Instant::now(); // Reset interacción al hacer click
                self.shader_click_time = std::time::Instant::now();
                
                // [GIRO PREDICTIVO] Registrar actividad del usuario
                crate::util::register_activity();
                
                // [FIX LSD] Restauración inmediata de opacidad al hacer clic
                if self.settings.theme.lsd_mode && !self.settings_state.is_open && !self.mods_state.is_open {
                    // Restaurar opacidad inmediatamente si está baja
                    if self.ui_opacity_accumulator < 0.9 {
                        self.ui_opacity_accumulator = 1.0;
                    }
                }

                // [Consolidation] MousePressed also acts as ShaderClicked now
                self.shader_click_intensity = 2.0;
                self.shader_click_time = std::time::Instant::now();
                self.last_mouse_move_time = std::time::Instant::now();
                Some(Task::none())
            }

            Message::MouseReleased => {
                self.is_mouse_pressed = false; // Resetear estado al soltar
                self.last_mouse_release_time = std::time::Instant::now(); // Registrar cuando se solto
                Some(Task::none())
            }

            Message::Tick(_now) => {
                let frame_time = self.start_time.elapsed().as_secs_f32();
                self.current_time = frame_time;

                if self.is_focused && !self.is_minimized {
                    crate::util::register_activity();
                }

                let is_modal_active = self.settings_state.is_open || self.mods_state.is_open;
                
                if let Some(ref mut lsd) = *self.lsd_shader_instance.borrow_mut() {
                    lsd.update_time(frame_time);
                }

                if !self.settings.theme.lsd_mode && !is_modal_active {
                    return Some(Task::none());
                }

                // [DEBUG LSD]
                if self.ui_opacity_accumulator < 0.1 {
                    use std::sync::LazyLock;
                    static LAST_LOW_FPS_LOG: LazyLock<std::sync::Mutex<std::time::Instant>> = 
                        LazyLock::new(|| std::sync::Mutex::new(std::time::Instant::now()));
                    
                    if let Ok(mut last_log) = LAST_LOW_FPS_LOG.lock() {
                        if last_log.elapsed().as_secs() > 5 {
                            *last_log = std::time::Instant::now();
                        }
                    }
                }

                if self.resizing_direction.is_some() {
                    return Some(Task::none());
                }

                let dt = 0.016; 
                let fade_speed = 1.5; 
                let reveal_speed = 2.5; 
                let hold_threshold = 0.25; 
                let inactivity_threshold = 10.0; 

                let elapsed_since_click = self.shader_click_time.elapsed().as_secs_f32();
                let elapsed_idle = self.last_mouse_move_time.elapsed().as_secs_f32();

                let is_modal_active = self.settings_state.is_open || self.mods_state.is_open;

                let tasks = Vec::new();

                if is_modal_active {
                    self.ui_opacity_accumulator =
                        (self.ui_opacity_accumulator + dt * reveal_speed).min(1.0);

                    if self.ui_opacity_accumulator > 0.1 && self.is_cursor_hidden {
                        self.is_cursor_hidden = false;
                    }
                } else {
                    let should_fade_out = (self.is_mouse_pressed
                        && elapsed_since_click > hold_threshold)
                        || elapsed_idle > inactivity_threshold + 0.5;

                    if should_fade_out {
                        self.ui_opacity_accumulator =
                            (self.ui_opacity_accumulator - dt * fade_speed).max(0.0);

                        if self.ui_opacity_accumulator < 0.05 && !self.is_cursor_hidden {
                            self.is_cursor_hidden = true;
                        }
                    } else {
                        self.ui_opacity_accumulator =
                            (self.ui_opacity_accumulator + dt * reveal_speed).min(1.0);

                        if self.ui_opacity_accumulator > 0.1 && self.is_cursor_hidden {
                            self.is_cursor_hidden = false;
                        }
                    }
                }

                let t = self.start_time.elapsed().as_secs_f32();
                let ox = (t * 1.3).sin() * 1.0 + (t * 2.8).cos() * 0.5 + (t * 0.7).sin() * 0.3;
                let oy = (t * 0.9).cos() * 1.0 + (t * 3.5).sin() * 0.5 + (t * 1.1).cos() * 0.3;
                self.lsd_offset = (ox, oy);

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

                if tasks.is_empty() {
                    Some(Task::none())
                } else {
                    Some(Task::batch(tasks))
                }
            }

            Message::ShaderClicked => {
                self.shader_click_intensity = 2.0; 
                self.shader_click_time = std::time::Instant::now();
                self.last_mouse_move_time = std::time::Instant::now();
                Some(Task::none())
            }

            Message::ResizePressed(dir) => {
                if self.is_wayland {
                    return Some(Task::none());
                }
                
                self.resizing_direction = Some(*dir);

                self.drag_start_window_pos = self.current_window_pos;
                self.drag_start_window_size = self.current_window_size;

                self.drag_start_mouse_screen_pos = Point::new(
                    self.current_window_pos.x + self.cursor_position.x,
                    self.current_window_pos.y + self.cursor_position.y,
                );

                Some(Task::none())
            }

            Message::ResizeReleased => {
                if self.is_wayland {
                    return Some(Task::none());
                }
                
                self.resizing_direction = None;
                self.is_mouse_pressed = false; 
                self.last_mouse_release_time = std::time::Instant::now(); 
                Some(Task::none())
            }

            Message::NextShader => {
                if self.settings.theme.lsd_mode && self.shader_transition <= 0.0 {
                    self.next_shader_idx =
                        (self.active_shader_idx + 1) % self.total_shaders_available;
                    self.shader_transition = 0.01;
                    self.shader_change_timer = 0.0;
                }
                Some(Task::none())
            }
            
            Message::WindowResized(size) => {
                self.current_window_size = *size;
                self.window_size = *size;
                self.news_section.update_viewport_height(size.height);
                self.settings.width = size.width as u32;
                self.settings.height = size.height as u32;

                if self.resizing_direction.is_none() {
                    let size_captured = *size;
                     Some(window::oldest().and_then(move |id| {
                        Task::batch(vec![
                            window::gain_focus(id),
                            window::is_maximized(id)
                                .map(move |max| Message::WindowResizedWithMaximized(size_captured, max)),
                        ])
                    }))
                } else {
                    Some(Task::none())
                }
            }

            Message::WindowResizedWithMaximized(size, is_maximized) => {
                self.window_size = *size;
                self.is_maximized = *is_maximized;
                self.settings.width = size.width as u32;
                self.settings.height = size.height as u32;
                self.settings_state.temp_settings.width = size.width as u32;
                self.settings_state.temp_settings.height = size.height as u32;
                Some(Task::none())
            }
            
             Message::WindowDrag => {
                if self.is_wayland {
                    return Some(Task::none());
                }
                
                let now = std::time::Instant::now();
                let duration = now.duration_since(self.last_title_click);
                self.last_title_click = now;

                if duration < std::time::Duration::from_millis(300) {
                    self.is_maximized = !self.is_maximized;
                    Some(window::oldest().and_then(|id| window::toggle_maximize(id)))
                } else {
                    Some(window::oldest().and_then(|id| window::drag(id)))
                }
            }
            
            Message::MinimizeWindow => {
                if self.is_wayland {
                    return Some(Task::none());
                }
                self.is_minimized = true;
                self.news_section.images.clear(); 
                Some(window::oldest().and_then(|id| {
                    Task::batch(vec![
                        window::minimize(id, true),
                        Task::perform(async {}, |_| {
                            crate::util::trim_memory_with_level(crate::util::TrimLevel::Aggressive);
                            Message::None
                        }),
                    ])
                }))
            }
            
            Message::MaximizeWindow => {
                self.is_maximized = !self.is_maximized;
                if self.is_minimized {
                    self.is_minimized = false;
                    let mut tasks = vec![window::oldest().and_then(|id| window::toggle_maximize(id))];
                    if self.settings.enable_news {
                        tasks.push(Task::done(Message::News(NewsMessage::ReloadImages)));
                    }
                    Some(Task::batch(tasks))
                } else {
                    Some(window::oldest().and_then(|id| window::toggle_maximize(id)))
                }
            }

            Message::ToggleFullscreen => {
                let entering_fullscreen = !self.is_fullscreen;
                self.is_fullscreen = entering_fullscreen;
                if entering_fullscreen {
                    self.is_maximized = false;
                }
                Some(window::oldest().and_then(move |id| {
                    if entering_fullscreen {
                        Task::batch(vec![
                            window::set_mode(id, window::Mode::Windowed),
                            window::set_mode(id, window::Mode::Fullscreen),
                        ])
                    } else {
                        window::set_mode(id, window::Mode::Windowed)
                    }
                }))
            }
            
            Message::WindowEvent(event) => {
                match event {
                    window::Event::Resized(size) => {
                        let delta_w = (size.width - self.current_window_size.width).abs();
                        let delta_h = (size.height - self.current_window_size.height).abs();

                        if self.resizing_direction.is_some() && delta_w < 5.0 && delta_h < 5.0 {
                            return Some(Task::none());
                        }

                        self.current_window_size = *size;
                        self.window_size = *size;

                        self.settings.width = size.width as u32;
                        self.settings.height = size.height as u32;
                        self.settings_state.temp_settings.width = size.width as u32;
                        self.settings_state.temp_settings.height = size.height as u32;

                        let size_clone = *size;
                        Some(window::oldest().and_then(move |id| {
                            Task::batch(vec![window::is_maximized(id).map(move |is_maximized| {
                                Message::WindowResizedWithMaximized(size_clone, is_maximized)
                            })])
                        }))
                    }
                    window::Event::Moved(point) => {
                        self.current_window_pos = *point;
                        Some(Task::none())
                    }
                    window::Event::CloseRequested => {
                        Some(Task::done(Message::CloseRequested))
                    }
                    window::Event::Focused => {
                        self.is_focused = true;
                        if self.settings.enable_news && !self.news_section.posts.is_empty() && self.news_section.images.is_empty() {
                            return Some(Task::done(Message::News(NewsMessage::ReloadImages)));
                        }
                        Some(Task::none())
                    }
                    window::Event::Unfocused => {
                        self.is_focused = false;
                        Some(Task::none())
                    }
                    _ => Some(Task::none())
                }
            }
            
            Message::CloseRequested => {
                 Some(self.save_and_exit())
            }
            
            Message::ToggleWindowVisibility => {
                 self.is_window_visible = !self.is_window_visible;
                 let visible = self.is_window_visible;
                 Some(window::oldest().and_then(move |id| {
                        if visible {
                            window::set_mode(id, window::Mode::Windowed)
                        } else {
                            window::set_mode(id, window::Mode::Hidden)
                        }
                 }))
            }

            Message::ToggleProfileDropdown => {
                 Some(self.handle_profile_message(Message::ToggleProfileDropdown))
            }
            
            _ => None
        }
    }
}

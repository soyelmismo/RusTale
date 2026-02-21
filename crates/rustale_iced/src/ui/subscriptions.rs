use crate::core::signals::FromCore; // ToCore kept if needed for traits, otherwise remove
use crate::messages::Message;
use iced::{Subscription, window};
// use tokio::sync::mpsc::Receiver; // Not needed if using Arc Mutex wrapper

pub fn listen_all(
    is_visible: bool,
    lsd_mode: bool,
    core_receiver: std::sync::Arc<std::sync::Mutex<Option<tokio::sync::mpsc::Receiver<FromCore>>>>
) -> Subscription<Message> {
    let mut subscriptions = Vec::new();

    // 1. Core Signals Listener (Delegated to bridge)
    subscriptions.push(crate::ui::bridge::subscribe(core_receiver));

    // 2. Window Events
    subscriptions.push(window::events().map(|(id, event)| Message::WindowEvent(id, event)));

    // 3. Tick / Animation Frame
    if is_visible {
        let fps = if lsd_mode { 60.0 } else { 30.0 };
        subscriptions.push(
            iced::time::every(std::time::Duration::from_secs_f64(1.0 / fps))
                .map(Message::Tick)
        );
    }
    
    // 4. Memory Stats (from original main.rs)
    subscriptions.push(
        iced::time::every(std::time::Duration::from_secs(5)).map(|_| {
            crate::util::check_auto_trim();
            Message::MemoryStatsUpdate
        })
    );

    Subscription::batch(subscriptions)
}

pub fn listen_keyboard() -> iced::Subscription<Message> {
    iced::event::listen_with(|event, _status, _window_id| {
        use iced::Event;
        use crate::messages::Message;
        use crate::ui::news_section::NewsMessage;

        if let Event::Keyboard(iced::keyboard::Event::KeyPressed { key, .. }) = event {
            match key {
                iced::keyboard::Key::Named(iced::keyboard::key::Named::ArrowRight) => Some(Message::NextShaderManual),
                iced::keyboard::Key::Character(s) if s.as_str() == "s" => Some(Message::NextShaderManual),
                iced::keyboard::Key::Named(iced::keyboard::key::Named::ArrowLeft) => Some(Message::NextShaderManual),
                iced::keyboard::Key::Character(a) if a.as_str() == "a" => Some(Message::NextShaderManual),
                iced::keyboard::Key::Named(iced::keyboard::key::Named::ArrowUp) => Some(Message::News(NewsMessage::ScrollOffsetChanged(-30.0))),
                iced::keyboard::Key::Named(iced::keyboard::key::Named::ArrowDown) => Some(Message::News(NewsMessage::ScrollOffsetChanged(30.0))),
                iced::keyboard::Key::Named(iced::keyboard::key::Named::PageUp) => Some(Message::News(NewsMessage::ScrollOffsetChanged(-300.0))),
                iced::keyboard::Key::Named(iced::keyboard::key::Named::PageDown) => Some(Message::News(NewsMessage::ScrollOffsetChanged(300.0))),
                iced::keyboard::Key::Named(iced::keyboard::key::Named::Home) => Some(Message::News(NewsMessage::ScrollOffsetChanged(f32::MIN))),
                iced::keyboard::Key::Named(iced::keyboard::key::Named::End) => Some(Message::News(NewsMessage::ScrollOffsetChanged(f32::MAX))),
                _ => None,
            }
        } else {
            None
        }
    })
}

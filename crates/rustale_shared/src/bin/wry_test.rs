use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::WindowBuilder;
use tao::event::{Event, WindowEvent};
use wry::WebViewBuilder;

fn main() {
    let event_loop = EventLoopBuilder::new().build();
    let window = WindowBuilder::new().build(&event_loop).unwrap();
    let _webview = WebViewBuilder::new()
        .with_url("https://google.com")
        .build(&window)
        .unwrap();
    
    println!("Compiled!");
}

use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    response::IntoResponse,
};

pub async fn handle_ws_upgrade(ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(handle_socket)
}

async fn handle_socket(mut socket: WebSocket) {
    // A simple loop to keep the socket alive and discard messages.
    // In a full implementation, this would handle Hytale social packets.
    while let Some(msg) = socket.recv().await {
        if let Ok(msg) = msg {
            match msg {
                Message::Close(_) => break,
                _ => {} // Ignore other messages for now
            }
        } else {
            break; // Client disconnected
        }
    }
}

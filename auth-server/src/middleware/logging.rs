use axum::{
    extract::Request,
    http::HeaderMap,
    middleware::Next,
    response::Response,
};
use std::collections::HashMap;

pub async fn log_request(req: Request, next: Next) -> Response {
    let path = req.uri().path().to_string();
    let method = req.method().clone();
    let headers = req.headers().clone();

    // Skip telemetry/analytics/event logging
    if !path.starts_with("/telemetry")
        && !path.starts_with("/analytics")
        && !path.starts_with("/event")
    {
        let filtered_headers = filter_sensitive_headers(&headers);
        println!(
            "Request: {} {}\\n    Headers: {:?}",
            method, path, filtered_headers
        );
    }

    next.run(req).await
}

fn filter_sensitive_headers(headers: &HeaderMap) -> HashMap<String, String> {
    let mut filtered = HashMap::new();
    
    for (name, value) in headers.iter() {
        let name_str = name.as_str();
        let mut val_str = value.to_str().unwrap_or("[invalid]").to_string();

        if name_str.eq_ignore_ascii_case("authorization") && val_str.starts_with("Bearer ") {
            if val_str.len() > 15 {
                val_str = format!("{}...", &val_str[..15]);
            }
        }
        filtered.insert(name_str.to_string(), val_str);
    }

    filtered
}

use axum::{
    extract::Request,
    http::{header, HeaderValue, StatusCode},
    middleware::Next,
    response::Response,
};

fn is_allowed_origin(origin: &str) -> bool {
    if origin == "null" {
        return false;
    }

    if let Ok(url) = url::Url::parse(origin) {
        if let Some(host) = url.host_str() {
            let clean_host = host.trim_matches(|c| c == '[' || c == ']');
            if clean_host == "localhost" || clean_host == "127.0.0.1" || clean_host == "::1" || clean_host.ends_with(".hytale.com") || clean_host == "hytale.com" {
                return true;
            }
        }
        if url.scheme() == "app" || url.scheme() == "tauri" {
            return true;
        }
    }

    false
}

pub async fn cors_middleware(request: Request, next: Next) -> Result<Response, StatusCode> {
    let origin_header = request
        .headers()
        .get(header::ORIGIN)
        .cloned();

    let mut response = next.run(request).await;
    
    let headers = response.headers_mut();

    headers.append(header::VARY, HeaderValue::from_static("origin"));

    if let Some(origin_val) = origin_header {
        if let Ok(origin_str) = origin_val.to_str() {
            if is_allowed_origin(origin_str) {
                headers.insert(
                    header::ACCESS_CONTROL_ALLOW_ORIGIN,
                    origin_val,
                );
            }
        }
    }
    
    // Allow specific headers
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("authorization, content-type"),
    );
    
    // Allow specific methods
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("GET, POST, PUT, DELETE, OPTIONS"),
    );
    
    Ok(response)
}

pub async fn catch_all_handler(method: axum::http::Method) -> StatusCode {
    if method == axum::http::Method::OPTIONS {
        StatusCode::OK
    } else {
        StatusCode::NOT_FOUND
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_allowed_origin() {
        assert!(is_allowed_origin("http://localhost"));
        assert!(is_allowed_origin("http://localhost:8080"));
        assert!(is_allowed_origin("http://127.0.0.1:3000"));
        assert!(is_allowed_origin("http://[::1]:8080"));
        assert!(is_allowed_origin("https://hytale.com"));
        assert!(is_allowed_origin("https://sub.hytale.com"));
        assert!(is_allowed_origin("app://localhost"));
        assert!(is_allowed_origin("tauri://localhost"));

        assert!(!is_allowed_origin("http://malicious.com"));
        assert!(!is_allowed_origin("http://fakehytale.com"));
        assert!(!is_allowed_origin("http://hytale.com.attacker.com"));
        assert!(!is_allowed_origin("null"));
        assert!(!is_allowed_origin("invalid_url"));
    }
}

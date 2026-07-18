use axum::{
    extract::Request,
    http::{header, HeaderValue, StatusCode},
    middleware::Next,
    response::Response,
};

pub async fn cors_middleware(request: Request, next: Next) -> Result<Response, StatusCode> {
    let mut response = next.run(request).await;
    
    let headers = response.headers_mut();
    
    // Allow any origin
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    
    // Allow specific headers
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("authorization, content-type"),
    );
    
    // Allow specific methods
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("GET, POST, PUT, DELETE"),
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

use crate::AppState;

use axum::{
    body::Body,
    extract::{Request, State},
    http::{Response, StatusCode, header},
};
use tracing::warn;

const LOOP_HEADER: &str = "x-ollama-audit-proxy";

pub async fn get_proxy_handler(
    State(state): State<AppState>,
    req: Request<Body>,
) -> Result<Response<Body>, StatusCode> {
    let (parts, _) = req.into_parts();

    if parts.headers.contains_key(LOOP_HEADER) {
        warn!("loop detected – request already passed through this proxy, dropping");
        return Err(StatusCode::LOOP_DETECTED);
    }

    let path = parts
        .uri
        .path_and_query()
        .map(|x| x.as_str())
        .unwrap_or("")
        .to_string();

    let url = format!("{}{}", state.ollama_url(), path);
    let mut req_builder = state.http().request(parts.method.clone(), url);

    for (key, value) in parts.headers.iter() {
        if key == header::HOST || key == header::CONNECTION {
            continue;
        }
        req_builder = req_builder.header(key, value);
    }
    req_builder = req_builder.header(LOOP_HEADER, "1");

    let resp = req_builder
        .send()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    let status = resp.status();
    let full = resp.bytes().await.map_err(|_| StatusCode::BAD_GATEWAY)?;
    let mut response = Response::new(Body::from(full));
    *response.status_mut() = status;
    return Ok(response);
}

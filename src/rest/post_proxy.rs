use crate::AUDIT_TARGET;
use crate::AppState;
use crate::build_generation_update_batch;
use crate::build_langfuse_start_batch;
use crate::langfuse_post_batch;
use crate::log_audit_request;
use crate::log_audit_response;
use crate::parse_input_value;
use crate::parse_ollama_output;
use crate::request_is_streaming;
use axum::{
    body::Body,
    extract::{Request, State},
    http::{Response, StatusCode, header},
};
use bytes::Bytes;
use futures_util::StreamExt;
use http_body_util::BodyExt;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tracing::warn;
use uuid::Uuid;

const LOOP_HEADER: &str = "x-ollama-audit-proxy";

pub async fn post_proxy_handler(
    State(state): State<AppState>,
    req: Request<Body>,
) -> Result<Response<Body>, StatusCode> {
    let (parts, body) = req.into_parts();

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

    let body_bytes = body
        .collect()
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?
        .to_bytes();

    let url = format!("{}{}", state.ollama_url(), path);
    let mut req_builder = state.http().request(parts.method.clone(), url);

    for (key, value) in parts.headers.iter() {
        if key == header::HOST
            || key == header::CONTENT_LENGTH
            || key == header::TRANSFER_ENCODING
            || key == header::CONNECTION
        {
            continue;
        }
        req_builder = req_builder.header(key, value);
    }
    req_builder = req_builder.header(LOOP_HEADER, "1");

    let resp = req_builder
        .body(body_bytes.clone())
        .send()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    let input_val = parse_input_value(&body_bytes);
    let model = input_val
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or("unknown")
        .to_string();

    let trace_id = Uuid::new_v4().to_string();
    let gen_id = Uuid::new_v4().to_string();

    if state.audit_log_always() || state.langfuse().is_none() {
        log_audit_request(&trace_id, &path, &model, &input_val);
    }
    if let Some(cfg) = state.langfuse() {
        let batch =
            build_langfuse_start_batch(&trace_id, &gen_id, &path, input_val.clone(), &model);
        let http = state.http().clone();
        let cfg = cfg.clone();
        let tid = trace_id.clone();
        let path_c = path.clone();
        let model_c = model.clone();
        let input_c = input_val.clone();
        let always = state.audit_log_always();
        tokio::spawn(async move {
            if let Err(e) = langfuse_post_batch(&http, &cfg, batch).await {
                warn!(target: AUDIT_TARGET, trace_id = %tid, "langfuse trace/generation create: {e}");
                if !always {
                    log_audit_request(&tid, &path_c, &model_c, &input_c);
                }
            }
        });
    }

    let status = resp.status();
    let streaming = request_is_streaming(&body_bytes);

    if streaming {
        let lf = state.langfuse().clone();
        let http = state.http().clone();
        let gid = gen_id.clone();
        let tid_stream = trace_id.clone();
        let audit_always = state.audit_log_always();
        let upstream_status = status.as_u16();
        let (tx, rx) = mpsc::channel::<Result<Bytes, std::io::Error>>(32);
        tokio::spawn(async move {
            let mut s = resp.bytes_stream();
            let mut buf: Vec<u8> = Vec::new();
            while let Some(item) = s.next().await {
                match item {
                    Ok(chunk) => {
                        buf.extend_from_slice(&chunk);
                        if tx.send(Ok(Bytes::from(chunk))).await.is_err() {
                            return;
                        }
                    }
                    Err(e) => {
                        let _ = tx
                            .send(Err(std::io::Error::new(
                                std::io::ErrorKind::Other,
                                e.to_string(),
                            )))
                            .await;
                        return;
                    }
                }
            }
            let out = parse_ollama_output(&buf);
            if audit_always {
                log_audit_response(&tid_stream, upstream_status, &out);
            }
            match lf {
                Some(cfg) => {
                    let batch = build_generation_update_batch(&gid, out.clone());
                    let tid_u = tid_stream.clone();
                    let always = audit_always;
                    tokio::spawn(async move {
                        if let Err(e) = langfuse_post_batch(&http, &cfg, batch).await {
                            warn!(target: AUDIT_TARGET, trace_id = %tid_u, "langfuse generation-update: {e}");
                            if !always {
                                log_audit_response(&tid_u, upstream_status, &out);
                            }
                        }
                    });
                }
                None => {
                    if !audit_always {
                        log_audit_response(&tid_stream, upstream_status, &out);
                    }
                }
            }
            drop(tx);
        });
        let body = Body::from_stream(ReceiverStream::new(rx));
        let mut response = Response::new(body);
        *response.status_mut() = status;
        return Ok(response);
    }

    let full = resp.bytes().await.map_err(|_| StatusCode::BAD_GATEWAY)?;

    let out = parse_ollama_output(&full);
    let upstream_status = status.as_u16();
    if state.audit_log_always() {
        log_audit_response(&trace_id, upstream_status, &out);
    }
    if let Some(cfg) = state.langfuse() {
        let batch = build_generation_update_batch(&gen_id, out.clone());
        let http = state.http().clone();
        let cfg = cfg.clone();
        let tid = trace_id.clone();
        let always = state.audit_log_always();
        tokio::spawn(async move {
            if let Err(e) = langfuse_post_batch(&http, &cfg, batch).await {
                warn!(target: AUDIT_TARGET, trace_id = %tid, "langfuse generation-update: {e}");
                if !always {
                    log_audit_response(&tid, upstream_status, &out);
                }
            }
        });
    } else if !state.audit_log_always() {
        log_audit_response(&trace_id, upstream_status, &out);
    }

    let mut response = Response::new(Body::from(full));
    *response.status_mut() = status;
    Ok(response)
}

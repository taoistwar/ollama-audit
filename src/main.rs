use axum::{
    Router,
    body::Body,
    extract::{Request, State},
    http::{Response, StatusCode},
    routing::post,
};
use bytes::Bytes;
use futures_util::StreamExt;
use http_body_util::BodyExt;
use reqwest::Client;
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tracing::{info, warn};
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{fmt, prelude::*, EnvFilter, Registry};
use uuid::Uuid;

#[derive(Clone)]
struct LangfuseConfig {
    base_url: String,
    public_key: String,
    secret_key: String,
}

#[derive(Clone)]
struct AppState {
    ollama_url: String,
    http: Client,
    langfuse: Option<LangfuseConfig>,
    /// 为 true 时无论 Langfuse 是否成功，都写入本地审计日志（target: ollama_audit）
    audit_log_always: bool,
}

fn env_flag_true(key: &str) -> bool {
    std::env::var(key)
        .ok()
        .map(|v| {
            matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

/// 初始化滚动文件日志（须持有返回的 `WorkerGuard` 直至进程结束，否则缓冲可能未刷盘）
fn init_tracing() -> tracing_appender::non_blocking::WorkerGuard {
    let log_dir = std::env::var("LOG_DIR").unwrap_or_else(|_| "logs".to_string());
    if let Err(e) = std::fs::create_dir_all(&log_dir) {
        eprintln!("failed to create LOG_DIR {log_dir:?}: {e}");
    }

    let rot = std::env::var("LOG_ROTATION").unwrap_or_default();
    let rot_key = rot.to_ascii_lowercase();
    let (rotation, rotation_label) = match rot_key.as_str() {
        "hourly" | "hour" => (Rotation::HOURLY, "hourly"),
        "minutely" | "minute" => (Rotation::MINUTELY, "minutely"),
        _ => (Rotation::DAILY, "daily"),
    };

    let file_appender = RollingFileAppender::new(rotation, &log_dir, "ollama-proxy");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let file_layer = fmt::layer()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_target(true);

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));

    let registry = Registry::default().with(filter).with(file_layer);

    if env_flag_true("LOG_DISABLE_STDOUT") {
        registry.init();
    } else {
        let stdout_layer = fmt::layer().with_writer(std::io::stdout).with_target(true);
        registry.with(stdout_layer).init();
    }

    info!(
        "rolling log file: directory={log_dir:?} prefix=ollama-proxy rotation={rotation_label}"
    );

    guard
}

fn ingestion_timestamp() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

async fn langfuse_post_batch(
    client: &Client,
    cfg: &LangfuseConfig,
    batch: Vec<serde_json::Value>,
) -> Result<(), String> {
    let url = format!(
        "{}/api/public/ingestion",
        cfg.base_url.trim_end_matches('/')
    );
    let body = serde_json::json!({ "batch": batch });
    let resp = client
        .post(url)
        .basic_auth(&cfg.public_key, Some(&cfg.secret_key))
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!("HTTP {status}: {text}"));
    }
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
        if let Some(errs) = v.get("errors").and_then(|e| e.as_array()) {
            if !errs.is_empty() {
                return Err(format!("langfuse ingestion partial errors: {v}"));
            }
        }
    }
    Ok(())
}

/// 本地审计日志：未启用 Langfuse、上报失败，或 `AUDIT_LOG_ALWAYS` 开启时写入
const AUDIT_TARGET: &str = "ollama_audit";

fn json_for_log(v: &serde_json::Value) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "<non-serializable>".to_string())
}

fn truncate_log_string(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}... [truncated, total {} chars]", &s[..end], s.len())
}

fn log_audit_request(trace_id: &str, path: &str, model: &str, input: &serde_json::Value) {
    let input_s = truncate_log_string(&json_for_log(input), 16_384);
    info!(
        target: AUDIT_TARGET,
        trace_id = %trace_id,
        path = %path,
        model = %model,
        input = %input_s,
        "audit request"
    );
}

fn log_audit_response(trace_id: &str, upstream_status: u16, output: &serde_json::Value) {
    let output_s = truncate_log_string(&json_for_log(output), 16_384);
    info!(
        target: AUDIT_TARGET,
        trace_id = %trace_id,
        upstream_status,
        output = %output_s,
        "audit response"
    );
}

fn parse_input_value(body_bytes: &[u8]) -> serde_json::Value {
    serde_json::from_slice(body_bytes).unwrap_or_else(|_| {
        serde_json::Value::String(String::from_utf8_lossy(body_bytes).into_owned())
    })
}

fn parse_ollama_output(raw: &[u8]) -> serde_json::Value {
    if raw.is_empty() {
        return serde_json::Value::Null;
    }
    if let Ok(v) = serde_json::from_slice::<serde_json::Value>(raw) {
        if let Some(c) = v.get("message").and_then(|m| m.get("content")) {
            return c.clone();
        }
        if let Some(r) = v.get("response") {
            return r.clone();
        }
        return v;
    }
    let mut text = String::new();
    for line in raw.split(|b| *b == b'\n') {
        if line.is_empty() {
            continue;
        }
        if let Ok(v) = serde_json::from_slice::<serde_json::Value>(line) {
            if let Some(c) = v.get("message").and_then(|m| m.get("content")).and_then(|c| c.as_str())
            {
                text.push_str(c);
            } else if let Some(r) = v.get("response").and_then(|r| r.as_str()) {
                text.push_str(r);
            }
        }
    }
    if text.is_empty() {
        serde_json::Value::String(String::from_utf8_lossy(raw).into_owned())
    } else {
        serde_json::Value::String(text)
    }
}

fn request_streaming(body_bytes: &[u8]) -> bool {
    serde_json::from_slice::<serde_json::Value>(body_bytes)
        .ok()
        .and_then(|v| v.get("stream").and_then(|x| x.as_bool()))
        .unwrap_or(false)
}

fn build_langfuse_start_batch(
    trace_id: &str,
    gen_id: &str,
    path: &str,
    input: serde_json::Value,
    model: &str,
) -> Vec<serde_json::Value> {
    let ts = ingestion_timestamp();
    vec![
        serde_json::json!({
            "id": Uuid::new_v4().to_string(),
            "timestamp": ts,
            "type": "trace-create",
            "body": {
                "id": trace_id,
                "timestamp": ts,
                "name": format!("ollama {path}"),
                "metadata": { "path": path, "source": "ollama-audit" },
                "input": input.clone(),
            }
        }),
        serde_json::json!({
            "id": Uuid::new_v4().to_string(),
            "timestamp": ts,
            "type": "generation-create",
            "body": {
                "id": gen_id,
                "traceId": trace_id,
                "name": "ollama",
                "startTime": ts,
                "model": model,
                "input": input,
            }
        }),
    ]
}

fn build_generation_update_batch(gen_id: &str, output: serde_json::Value) -> Vec<serde_json::Value> {
    let ts = ingestion_timestamp();
    vec![serde_json::json!({
        "id": Uuid::new_v4().to_string(),
        "timestamp": ts,
        "type": "generation-update",
        "body": {
            "id": gen_id,
            "endTime": ts,
            "output": output,
        }
    })]
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let _log_guard = init_tracing();

    let ollama_url = std::env::var("OLLAMA_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:11434".to_string())
        .trim_end_matches('/')
        .to_string();

    let bind = std::env::var("BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:5000".to_string());
    let addr: SocketAddr = bind
        .parse()
        .unwrap_or_else(|e| panic!("invalid BIND_ADDR {bind:?}: {e}"));

    let langfuse = match (
        std::env::var("LANGFUSE_PUBLIC_KEY")
            .ok()
            .filter(|s| !s.is_empty()),
        std::env::var("LANGFUSE_SECRET_KEY")
            .ok()
            .filter(|s| !s.is_empty()),
    ) {
        (Some(public_key), Some(secret_key)) => {
            let base_url = std::env::var("LANGFUSE_BASE_URL")
                .unwrap_or_else(|_| "https://cloud.langfuse.com".to_string());
            info!("Langfuse ingestion enabled ({})", base_url.trim_end_matches('/'));
            Some(LangfuseConfig {
                base_url,
                public_key,
                secret_key,
            })
        }
        _ => {
            info!("Langfuse disabled: set LANGFUSE_PUBLIC_KEY and LANGFUSE_SECRET_KEY");
            None
        }
    };

    let audit_log_always = env_flag_true("AUDIT_LOG_ALWAYS");
    if audit_log_always {
        info!("AUDIT_LOG_ALWAYS: local audit logs (target ollama_audit) on every request/response");
    }

    let state = AppState {
        ollama_url,
        http: Client::new(),
        langfuse,
        audit_log_always,
    };

    let app = Router::new()
        .route("/{*path}", post(proxy_handler))
        .with_state(state);

    info!("Proxy running at http://{}", addr);

    let listener = TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn proxy_handler(
    State(state): State<AppState>,
    req: Request<Body>,
) -> Result<Response<Body>, StatusCode> {
    let (parts, body) = req.into_parts();

    let body_bytes = body
        .collect()
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?
        .to_bytes();

    let input_val = parse_input_value(&body_bytes);
    let model = input_val
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or("unknown")
        .to_string();

    let path = parts
        .uri
        .path_and_query()
        .map(|x| x.as_str())
        .unwrap_or("")
        .to_string();

    let trace_id = Uuid::new_v4().to_string();
    let gen_id = Uuid::new_v4().to_string();

    if state.audit_log_always || state.langfuse.is_none() {
        log_audit_request(&trace_id, &path, &model, &input_val);
    }
    if let Some(ref cfg) = state.langfuse {
        let batch = build_langfuse_start_batch(&trace_id, &gen_id, &path, input_val.clone(), &model);
        let http = state.http.clone();
        let cfg = cfg.clone();
        let tid = trace_id.clone();
        let path_c = path.clone();
        let model_c = model.clone();
        let input_c = input_val.clone();
        let always = state.audit_log_always;
        tokio::spawn(async move {
            if let Err(e) = langfuse_post_batch(&http, &cfg, batch).await {
                warn!(target: AUDIT_TARGET, trace_id = %tid, "langfuse trace/generation create: {e}");
                if !always {
                    log_audit_request(&tid, &path_c, &model_c, &input_c);
                }
            }
        });
    }

    let url = format!("{}{}", state.ollama_url, path);
    let mut req_builder = state.http.post(url);

    for (key, value) in parts.headers.iter() {
        req_builder = req_builder.header(key, value);
    }

    let resp = req_builder
        .body(body_bytes.clone())
        .send()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    let status = resp.status();
    let streaming = request_streaming(&body_bytes);

    if streaming {
        let lf = state.langfuse.clone();
        let http = state.http.clone();
        let gid = gen_id.clone();
        let tid_stream = trace_id.clone();
        let audit_always = state.audit_log_always;
        let upstream_status = status.as_u16();
        let (tx, rx) = mpsc::channel::<Result<Bytes, std::io::Error>>(32);
        tokio::spawn(async move {
            let mut s = resp.bytes_stream();
            let mut buf: Vec<u8> = Vec::new();
            while let Some(item) = s.next().await {
                match item {
                    Ok(chunk) => {
                        buf.extend_from_slice(&chunk);
                        if tx.send(Ok(chunk)).await.is_err() {
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

    let full = resp
        .bytes()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    let out = parse_ollama_output(&full);
    let upstream_status = status.as_u16();
    if state.audit_log_always {
        log_audit_response(&trace_id, upstream_status, &out);
    }
    if let Some(ref cfg) = state.langfuse {
        let batch = build_generation_update_batch(&gen_id, out.clone());
        let http = state.http.clone();
        let cfg = cfg.clone();
        let tid = trace_id.clone();
        let always = state.audit_log_always;
        tokio::spawn(async move {
            if let Err(e) = langfuse_post_batch(&http, &cfg, batch).await {
                warn!(target: AUDIT_TARGET, trace_id = %tid, "langfuse generation-update: {e}");
                if !always {
                    log_audit_response(&tid, upstream_status, &out);
                }
            }
        });
    } else if !state.audit_log_always {
        log_audit_response(&trace_id, upstream_status, &out);
    }

    let mut response = Response::new(Body::from(full));
    *response.status_mut() = status;
    Ok(response)
}

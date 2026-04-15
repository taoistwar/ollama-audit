use reqwest::Client;

use crate::LangfuseConfig;
use crate::ingestion_timestamp;
use uuid::Uuid;
pub async fn langfuse_post_batch(
    client: &Client,
    cfg: &LangfuseConfig,
    batch: Vec<serde_json::Value>,
) -> Result<(), String> {
    let url = format!(
        "{}/api/public/ingestion",
        cfg.base_url().trim_end_matches('/')
    );
    let body = serde_json::json!({ "batch": batch });
    let resp = client
        .post(url)
        .basic_auth(&cfg.public_key(), Some(&cfg.secret_key()))
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

pub fn build_langfuse_start_batch(
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

pub fn build_generation_update_batch(
    gen_id: &str,
    output: serde_json::Value,
) -> Vec<serde_json::Value> {
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

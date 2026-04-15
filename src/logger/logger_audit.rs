use crate::AUDIT_TARGET;
use crate::json_for_log;
use tracing::info;

pub fn truncate_log_string(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}... [truncated, total {} chars]", &s[..end], s.len())
}

pub fn log_audit_request(trace_id: &str, path: &str, model: &str, input: &serde_json::Value) {
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
pub fn log_audit_response(trace_id: &str, upstream_status: u16, output: &serde_json::Value) {
    let output_s: String = truncate_log_string(&json_for_log(output), 16_384);
    info!(
        target: AUDIT_TARGET,
        trace_id = %trace_id,
        upstream_status,
        output = %output_s,
        "audit response"
    );
}

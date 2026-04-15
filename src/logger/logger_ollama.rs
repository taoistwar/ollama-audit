pub fn parse_ollama_output(raw: &[u8]) -> serde_json::Value {
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
            if let Some(c) = v
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_str())
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

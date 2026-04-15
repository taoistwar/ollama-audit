use crate::env_flag_true;
use reqwest::{Client, redirect};
use tracing::info;

#[derive(Clone)]
pub struct LangfuseConfig {
    base_url: String,
    public_key: String,
    secret_key: String,
}

impl LangfuseConfig {
    pub fn factory() -> Option<Self> {
        let public_key = std::env::var("LANGFUSE_PUBLIC_KEY")
            .ok()
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        let secret_key = std::env::var("LANGFUSE_SECRET_KEY")
            .ok()
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        let base_url = std::env::var("LANGFUSE_BASE_URL")
            .unwrap_or_else(|_| "https://cloud.langfuse.com".to_string());

        match (public_key, secret_key) {
            (Some(public_key), Some(secret_key)) => {
                info!(
                    "Langfuse ingestion enabled ({})",
                    base_url.trim_end_matches('/')
                );
                Some(LangfuseConfig::new(base_url, public_key, secret_key))
            }
            _ => {
                info!("Langfuse disabled: set LANGFUSE_PUBLIC_KEY and LANGFUSE_SECRET_KEY");
                None
            }
        }
    }
    pub fn new(base_url: String, public_key: String, secret_key: String) -> Self {
        Self {
            base_url,
            public_key,
            secret_key,
        }
    }
    pub fn base_url(&self) -> &str {
        &self.base_url
    }
    pub fn public_key(&self) -> &str {
        &self.public_key
    }
    pub fn secret_key(&self) -> &str {
        &self.secret_key
    }
    pub fn set_base_url(&mut self, base_url: String) {
        self.base_url = base_url;
    }
    pub fn set_public_key(&mut self, public_key: String) {
        self.public_key = public_key;
    }
    pub fn set_secret_key(&mut self, secret_key: String) {
        self.secret_key = secret_key;
    }
}

#[derive(Clone)]
pub struct AppState {
    ollama_url: String,
    http: Client,
    langfuse: Option<LangfuseConfig>,
    /// 为 true 时无论 Langfuse 是否成功，都写入本地审计日志（target: ollama_audit）
    audit_log_always: bool,
}

impl AppState {
    pub fn factory() -> Self {
        let ollama_url = std::env::var("OLLAMA_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:11434".to_string())
            .trim_end_matches('/')
            .to_string();

        let langfuse = LangfuseConfig::factory();

        let audit_log_always = env_flag_true("AUDIT_LOG_ALWAYS");
        if audit_log_always {
            info!(
                "AUDIT_LOG_ALWAYS: local audit logs (target ollama_audit) on every request/response"
            );
        }
        Self {
            ollama_url,
            http: Client::builder()
                .redirect(redirect::Policy::none())
                .build()
                .expect("failed to build HTTP client"),
            langfuse,
            audit_log_always,
        }
    }
    pub fn new(
        ollama_url: String,
        http: Client,
        langfuse: Option<LangfuseConfig>,
        audit_log_always: bool,
    ) -> Self {
        Self {
            ollama_url,
            http,
            langfuse,
            audit_log_always,
        }
    }
    pub fn ollama_url(&self) -> &str {
        &self.ollama_url
    }
    pub fn http(&self) -> &Client {
        &self.http
    }
    pub fn langfuse(&self) -> &Option<LangfuseConfig> {
        &self.langfuse
    }
    pub fn audit_log_always(&self) -> bool {
        self.audit_log_always
    }
    pub fn set_ollama_url(&mut self, ollama_url: String) {
        self.ollama_url = ollama_url;
    }
    pub fn set_http(&mut self, http: Client) {
        self.http = http;
    }
    pub fn set_langfuse(&mut self, langfuse: Option<LangfuseConfig>) {
        self.langfuse = langfuse;
    }
    pub fn set_audit_log_always(&mut self, audit_log_always: bool) {
        self.audit_log_always = audit_log_always;
    }
}

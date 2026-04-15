#[tokio::main]
async fn main() {
    ollama_audit::start_server().await;
}

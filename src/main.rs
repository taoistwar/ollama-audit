#[tokio::main]
async fn main() {
    llm_audit::start_server().await;
}

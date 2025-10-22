mod llm;
mod chat;
mod tools;
mod limiter;
mod executor;
mod parser;

use chat::ChatSession;
use dotenv::dotenv;
use std::env;

#[tokio::main]
async fn main() {
    dotenv().ok();
    
    let project_path = env::var("PROJECT_PATH").unwrap_or_else(|_| ".".to_string());
    
    let mut session = match ChatSession::new(project_path).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to initialize: {}", e);
            return;
        }
    };
    
    session.run().await;
}

use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::env;

pub trait LLMProvider {
    async fn generate(
        &self,
        prompt: &str,
        params: &serde_json::Value,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error>>;
}

pub struct AzureOpenAIClient {
    client: Client,
    endpoint: String,
    api_key: String,
    deployment: String,
    api_version: String,
}

#[derive(Serialize)]
struct AzureRequest {
    messages: Vec<AzureMessage>,
    max_tokens: u32,
    temperature: f32,
}

#[derive(Serialize)]
struct AzureMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct AzureResponse {
    choices: Vec<AzureChoice>,
}

#[derive(Deserialize)]
struct AzureChoice {
    message: AzureMessageResponse,
}

#[derive(Deserialize)]
struct AzureMessageResponse {
    content: String,
}

impl AzureOpenAIClient {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let endpoint = env::var("AZURE_OPENAI_ENDPOINT")?;
        let api_key = env::var("AZURE_OPENAI_KEY")?;
        let deployment = env::var("AZURE_OPENAI_DEPLOYMENT")?;
        let api_version = env::var("AZURE_OPENAI_API_VERSION")
            .unwrap_or_else(|_| "2024-08-01-preview".to_string());
        
        Ok(Self {
            client: Client::new(),
            endpoint,
            api_key,
            deployment,
            api_version,
        })
    }
}

impl LLMProvider for AzureOpenAIClient {
    async fn generate(
        &self,
        prompt: &str,
        _params: &serde_json::Value,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        let url = format!(
            "{}/openai/deployments/{}/chat/completions?api-version={}",
            self.endpoint.trim_end_matches('/'),
            self.deployment,
            self.api_version
        );
        
        let request_body = AzureRequest {
            messages: vec![AzureMessage {
                role: "user".to_string(),
                content: prompt.to_string(),
            }],
            max_tokens: 4096,
            temperature: 0.7,
        };
        
        let response = self
            .client
            .post(&url)
            .header("api-key", &self.api_key)
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await?;
        
        let azure_response: AzureResponse = response.json().await?;
        
        let content = azure_response
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .unwrap_or_default();
        
        Ok(serde_json::json!(content))
    }
}

use async_trait::async_trait;
use dotenvy::dotenv;
use log::{error, info};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[async_trait]
pub trait LLMProvider: Send + Sync {
    async fn generate(
        &self,
        prompt: &str,
        config: &Value,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>>;
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AzureOpenAIConfig {
    pub endpoint: String,
    pub api_key: String,
    pub api_version: String,
    pub deployment: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChatCompletionRequest {
    pub messages: Vec<ChatMessage>,
    pub temperature: f32,
    pub max_tokens: Option<u32>,
    pub top_p: f32,
    pub frequency_penalty: f32,
    pub presence_penalty: f32,
    pub model: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub choices: Vec<ChatChoice>,
    pub usage: Usage,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChatChoice {
    pub index: u32,
    pub message: ChatMessage,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

pub struct AzureOpenAIClient {
    config: AzureOpenAIConfig,
    client: Client,
}

impl AzureOpenAIClient {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        dotenv().ok();

        let endpoint = std::env::var("LLM_URL").map_err(|_| "LLM_URL not set")?;
        let api_key = std::env::var("LLM_KEY").map_err(|_| "LLM_KEY not set")?;
        let api_version =
            std::env::var("LLM_VERSION").unwrap_or_else(|_| "2023-12-01-preview".to_string());
        let deployment =
            std::env::var("LLM_MODEL").unwrap_or_else(|_| "DeepSeek-V3-0324".to_string());

        let config = AzureOpenAIConfig {
            endpoint,
            api_key,
            api_version,
            deployment,
        };

        Ok(Self {
            config,
            client: Client::new(),
        })
    }

    pub async fn chat_completions(
        &self,
        messages: Vec<ChatMessage>,
        temperature: f32,
        max_tokens: Option<u32>,
    ) -> Result<ChatCompletionResponse, Box<dyn std::error::Error>> {
        let url = format!(
            "{}/chat/completions?api-version=2024-05-01-preview",
            self.config.endpoint
        );

        let request_body = ChatCompletionRequest {
            messages,
            temperature,
            max_tokens,
            top_p: 1.0,
            frequency_penalty: 0.0,
            presence_penalty: 0.0,
            model: self.config.deployment.clone(),
        };

        info!("Sending request to Azure OpenAI: {}", url);

        let response = self
            .client
            .post(&url)
            .header("api-key", &self.config.api_key)
            .header("Content-Type", "appli`tion/json")
            .json(&request_body)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            error!("Azure OpenAI API error: {}", error_text);
            return Err(format!("Azure OpenAI API error: {}", error_text).into());
        }

        let completion_response: ChatCompletionResponse = response.json().await?;
        Ok(completion_response)
    }

    pub async fn simple_chat(&self, prompt: &str) -> Result<String, Box<dyn std::error::Error>> {
        let messages = vec![
            ChatMessage {
                role: "system".to_string(),
                content: "You are a helpful assistant.".to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: prompt.to_string(),
            },
        ];

        let response = self.chat_completions(messages, 0.7, Some(6000)).await?;

        if let Some(choice) = response.choices.first() {
            Ok(choice.message.content.clone())
        } else {
            Err("No response from AI".into())
        }
    }
}

#[async_trait]
impl LLMProvider for AzureOpenAIClient {
    async fn generate(
        &self,
        prompt: &str,
        _config: &Value,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        info!("Generating response using Azure OpenAI...");
        info!("Prompt length: {} characters", prompt.len());

        match self.simple_chat(prompt).await {
            Ok(content) => {
                info!("Received content successfully");
                Ok(content)
            }
            Err(e) => {
                // Convert the error into a Send + Sync boxed error by
                // flattening it to a string and creating an std::io::Error.
                let err = std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Azure OpenAI generation failed: {}", e),
                );
                let boxed_err: Box<dyn std::error::Error + Send + Sync> = Box::new(err);
                error!("Failed to generate content: {}", boxed_err);
                Err(boxed_err)
            }
        }
    }
}

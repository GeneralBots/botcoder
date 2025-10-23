use async_trait::async_trait;
use dotenvy::dotenv;
use log::{error, info};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

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

pub struct RateLimiter {
    max_tpm: u32,
    requests: Arc<Mutex<VecDeque<(Instant, u32)>>>,
    total_tokens_used: Arc<Mutex<u32>>,
}

impl RateLimiter {
    pub fn new(max_tpm: u32) -> Self {
        Self {
            max_tpm,
            requests: Arc::new(Mutex::new(VecDeque::new())),
            total_tokens_used: Arc::new(Mutex::new(0)),
        }
    }

    pub async fn wait_if_needed(&self, estimated_tokens: u32) {
        let mut requests = self.requests.lock().await;
        let now = Instant::now();
        let one_minute_ago = now - Duration::from_secs(60);

        while let Some((time, _)) = requests.front() {
            if *time < one_minute_ago {
                requests.pop_front();
            } else {
                break;
            }
        }

        let current_tpm: u32 = requests.iter().map(|(_, tokens)| tokens).sum();

        if current_tpm + estimated_tokens > self.max_tpm {
            if let Some((oldest_time, _)) = requests.front() {
                let elapsed = now.duration_since(*oldest_time);
                if elapsed < Duration::from_secs(60) {
                    let wait_time = Duration::from_secs(60) - elapsed + Duration::from_millis(100);
                    info!("TPM limit reached, waiting {}ms", wait_time.as_millis());
                    tokio::time::sleep(wait_time).await;

                    let now = Instant::now();
                    let one_minute_ago = now - Duration::from_secs(60);
                    requests.retain(|(time, _)| *time >= one_minute_ago);
                }
            }
        }

        requests.push_back((now, estimated_tokens));
        *self.total_tokens_used.lock().await += estimated_tokens;
    }

    pub async fn get_current_tpm(&self) -> u32 {
        let requests = self.requests.lock().await;
        let now = Instant::now();
        let one_minute_ago = now - Duration::from_secs(60);

        requests
            .iter()
            .filter(|(time, _)| *time >= one_minute_ago)
            .map(|(_, tokens)| tokens)
            .sum()
    }

    pub async fn get_total_tokens(&self) -> u32 {
        *self.total_tokens_used.lock().await
    }
}

pub struct AzureOpenAIClient {
    config: AzureOpenAIConfig,
    client: Client,
    rate_limiter: Arc<RateLimiter>,
}

impl AzureOpenAIClient {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        dotenv().ok();

        let endpoint = std::env::var("LLM_URL").map_err(|_| "LLM_URL not set")?;
        let api_key = std::env::var("LLM_KEY").map_err(|_| "LLM_KEY not set")?;
        let api_version =
            std::env::var("LLM_VERSION").unwrap_or_else(|_| "2024-05-01-preview".to_string());
        let deployment = std::env::var("LLM_MODEL").unwrap_or_else(|_| "gpt-4".to_string());

        let tpm_limit: u32 = std::env::var("LLM_TPM")
            .unwrap_or_else(|_| "20000".to_string())
            .parse()
            .unwrap_or(20000);

        let config = AzureOpenAIConfig {
            endpoint,
            api_key,
            api_version,
            deployment,
        };

        Ok(Self {
            config,
            client: Client::new(),
            rate_limiter: Arc::new(RateLimiter::new(tpm_limit)),
        })
    }

    pub fn get_rate_limiter(&self) -> Arc<RateLimiter> {
        self.rate_limiter.clone()
    }

    pub async fn chat_completions(
        &self,
        messages: Vec<ChatMessage>,
        temperature: f32,
        max_tokens: Option<u32>,
    ) -> Result<ChatCompletionResponse, Box<dyn std::error::Error>> {
        let url = format!(
            "{}/chat/completions?api-version={}",
            self.config.endpoint, self.config.api_version
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

        let estimated_tokens = request_body
            .messages
            .iter()
            .map(|msg| msg.content.len() / 4)
            .sum::<usize>() as u32
            + 100;

        self.rate_limiter.wait_if_needed(estimated_tokens).await;

        info!("Sending request to Azure OpenAI");

        let response = self
            .client
            .post(&url)
            .header("api-key", &self.config.api_key)
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            error!("Azure OpenAI API error: {}", error_text);
            return Err(format!("API error: {}", error_text).into());
        }

        let completion_response: ChatCompletionResponse = response.json().await?;

        if let Some(usage) = Some(&completion_response.usage) {
            let actual_tokens = usage.total_tokens;
            info!("Actual token usage: {}", actual_tokens);

            let mut requests = self.rate_limiter.requests.lock().await;
            if let Some(back) = requests.back_mut() {
                back.1 = actual_tokens;
            }
        }

        Ok(completion_response)
    }

    pub async fn simple_chat(&self, prompt: &str) -> Result<String, Box<dyn std::error::Error>> {
        let messages = vec![
            ChatMessage {
                role: "system".to_string(),
                content: "You are a helpful AI coding assistant.".to_string(),
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
            Err(" No response from AI".into())
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
        info!("Generating response...");

        match self.simple_chat(prompt).await {
            Ok(content) => Ok(content),
            Err(e) => {
                let err = std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Generation failed: {}", e),
                );
                Err(Box::new(err))
            }
        }
    }
}

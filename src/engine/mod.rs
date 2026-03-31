pub mod cost_tracker;

use anyhow::{Context, Result};
use async_openai::{
    types::{ChatCompletionRequestUserMessageArgs, CreateChatCompletionRequestArgs},
    Client,
};
use tracing::info;

// use crate::models::{Message, Role};

pub enum ModelProvider {
    OpenAI,
    Gemini,
}

pub struct QueryEngine {
    client: Client<async_openai::config::OpenAIConfig>,
    model: String,
}

impl QueryEngine {
    /// Create a new QueryEngine specifying the provider (OpenAI or Gemini)
    pub fn new(model: impl Into<String>, provider: ModelProvider) -> Self {
        let config = match provider {
            ModelProvider::OpenAI => {
                // Uses OPENAI_API_KEY by default
                async_openai::config::OpenAIConfig::default()
            }
            ModelProvider::Gemini => {
                // Uses GEMINI_API_KEY and Google's OpenAI compatible endpoint
                let api_key = std::env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY must be set");
                async_openai::config::OpenAIConfig::default()
                    .with_api_key(api_key)
                    .with_api_base("https://generativelanguage.googleapis.com/v1beta/openai/")
            }
        };

        let client = Client::with_config(config);
        
        Self {
            client,
            model: model.into(),
        }
    }

    /// Evaluates a query through the LLM, managing conversation history and tools.
    pub async fn query(&self, input: &str) -> Result<String> {
        info!("Sending query to OpenAI model: {}", self.model);
        
        let req = CreateChatCompletionRequestArgs::default()
            .max_tokens(1024u16)
            .model(&self.model)
            .messages([
                ChatCompletionRequestUserMessageArgs::default()
                    .content(input)
                    .build()?
                    .into()
            ])
            .build()
            .context("Failed to construct Chat Request")?;

        let response = self.client.chat().create(req).await?;
        
        let content = response
            .choices
            .first()
            .and_then(|c| c.message.content.clone())
            .unwrap_or_else(|| "<empty response>".to_string());
            
        Ok(content)
    }
}

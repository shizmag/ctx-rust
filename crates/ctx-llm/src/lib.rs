use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Gemini,
    OpenAi,
    Anthropic,
}

pub struct LlmClient {
    provider: Provider,
    api_key: String,
    model: String,
    base_url: Option<String>,
}

impl LlmClient {
    pub fn new(provider: Provider, api_key: String, model: String) -> Self {
        Self {
            provider,
            api_key,
            model,
            base_url: None,
        }
    }

    pub fn with_base_url(mut self, base_url: String) -> Self {
        self.base_url = Some(base_url);
        self
    }

    pub fn generate_response(&self, prompt: &str) -> Result<String, Box<dyn std::error::Error>> {
        match self.provider {
            Provider::OpenAi => self.call_openai(prompt),
            Provider::Anthropic => self.call_anthropic(prompt),
            Provider::Gemini => self.call_gemini(prompt),
        }
    }

    fn call_openai(&self, prompt: &str) -> Result<String, Box<dyn std::error::Error>> {
        #[derive(Serialize)]
        struct Message {
            role: String,
            content: String,
        }
        #[derive(Serialize)]
        struct Request {
            model: String,
            messages: Vec<Message>,
        }
        #[derive(Deserialize)]
        struct ResponseMessage {
            content: String,
        }
        #[derive(Deserialize)]
        struct Choice {
            message: ResponseMessage,
        }
        #[derive(Deserialize)]
        struct Response {
            choices: Vec<Choice>,
        }

        let body = Request {
            model: self.model.clone(),
            messages: vec![Message {
                role: "user".to_string(),
                content: prompt.to_string(),
            }],
        };

        let url = self.base_url.clone().unwrap_or_else(|| "https://api.openai.com/v1/chat/completions".to_string());

        let resp: Response = ureq::post(&url)
            .set("Authorization", &format!("Bearer {}", self.api_key))
            .set("Content-Type", "application/json")
            .send_json(&body)?
            .into_json()?;

        let content = resp
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .ok_or("OpenAI response was empty")?;

        Ok(content)
    }

    fn call_anthropic(&self, prompt: &str) -> Result<String, Box<dyn std::error::Error>> {
        #[derive(Serialize)]
        struct Message {
            role: String,
            content: String,
        }
        #[derive(Serialize)]
        struct Request {
            model: String,
            max_tokens: u32,
            messages: Vec<Message>,
        }
        #[derive(Deserialize)]
        struct ContentPart {
            text: String,
        }
        #[derive(Deserialize)]
        struct Response {
            content: Vec<ContentPart>,
        }

        let body = Request {
            model: self.model.clone(),
            max_tokens: 4096,
            messages: vec![Message {
                role: "user".to_string(),
                content: prompt.to_string(),
            }],
        };

        let url = self.base_url.clone().unwrap_or_else(|| "https://api.anthropic.com/v1/messages".to_string());

        let resp: Response = ureq::post(&url)
            .set("x-api-key", &self.api_key)
            .set("anthropic-version", "2023-06-01")
            .set("Content-Type", "application/json")
            .send_json(&body)?
            .into_json()?;

        let content = resp
            .content
            .first()
            .map(|c| c.text.clone())
            .ok_or("Anthropic response was empty")?;

        Ok(content)
    }

    fn call_gemini(&self, prompt: &str) -> Result<String, Box<dyn std::error::Error>> {
        #[derive(Serialize)]
        struct Part {
            text: String,
        }
        #[derive(Serialize)]
        struct Content {
            parts: Vec<Part>,
        }
        #[derive(Serialize)]
        struct Request {
            contents: Vec<Content>,
        }
        #[derive(Deserialize)]
        struct ResponsePart {
            text: String,
        }
        #[derive(Deserialize)]
        struct ResponseContent {
            parts: Vec<ResponsePart>,
        }
        #[derive(Deserialize)]
        struct Candidate {
            content: ResponseContent,
        }
        #[derive(Deserialize)]
        struct Response {
            candidates: Vec<Candidate>,
        }

        let body = Request {
            contents: vec![Content {
                parts: vec![Part {
                    text: prompt.to_string(),
                }],
            }],
        };

        let url = match &self.base_url {
            Some(base) => format!("{}/{}:generateContent?key={}", base, self.model, self.api_key),
            None => format!("https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}", self.model, self.api_key),
        };

        let resp: Response = ureq::post(&url)
            .set("Content-Type", "application/json")
            .send_json(&body)?
            .into_json()?;

        let content = resp
            .candidates
            .first()
            .and_then(|c| c.content.parts.first())
            .map(|p| p.text.clone())
            .ok_or("Gemini response was empty")?;

        Ok(content)
    }
}

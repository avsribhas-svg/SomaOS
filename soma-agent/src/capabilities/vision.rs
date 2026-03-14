use crate::capabilities::{param, Capability};
use base64::Engine;
use serde_json::{json, Value};
use soma_common::{CapabilityError, ErrorReason, ActionSchema, CapabilityResult};
use std::fs;

/// Vision capability — analyzes images using Ollama's multimodal model (qwen2.5-vl).
pub struct VisionCapability;

impl Capability for VisionCapability {
    fn name(&self) -> &str {
        "vision"
    }

    fn description(&self) -> &str {
        "AI vision model — analyze images and screenshots using multimodal LLM (qwen2.5-vl)"
    }

    fn actions(&self) -> Vec<ActionSchema> {
        vec![ActionSchema {
            name: "analyze_image".to_string(),
            description:
                "Analyze an image file or base64 data and describe its contents using qwen2.5-vl"
                    .to_string(),
            params: vec![
                param(
                    "path",
                    "string",
                    false,
                    "Path to image file (png/jpg/gif/bmp/webp)",
                ),
                param("base64", "string", false, "Base64-encoded image data"),
                param(
                    "prompt",
                    "string",
                    false,
                    "Specific question about the image (default: describe all content)",
                ),
            ],
        }]
    }

    fn execute(&self, action: &str, params: &Value) -> CapabilityResult {
        match action {
            "analyze_image" => self.analyze_image(params),
            _ => CapabilityResult {
                success: false,
                data: Value::Null,
                error: Some(CapabilityError::new(ErrorReason::UnknownAction, format!("Unknown vision action: {}", action))),
            },
        }
    }
}

impl VisionCapability {
    fn analyze_image(&self, params: &Value) -> CapabilityResult {
        let prompt = params["prompt"].as_str().unwrap_or(
            "Describe this image in detail. List all visible elements, text, UI components, and layout.",
        );

        let image_b64 = if let Some(b64) = params["base64"].as_str() {
            b64.to_string()
        } else if let Some(path) = params["path"].as_str() {
            let path = expand_tilde(path);
            match fs::read(&path) {
                Ok(bytes) => base64::engine::general_purpose::STANDARD.encode(&bytes),
                Err(e) => {
                    return CapabilityResult {
                        success: false,
                        data: Value::Null,
                        error: Some(CapabilityError::new(ErrorReason::NotFound, format!("Cannot read image '{}': {}", path, e))),
                    }
                }
            }
        } else {
            return CapabilityResult {
                success: false,
                data: Value::Null,
                error: Some(CapabilityError::new(ErrorReason::MissingParam, "Must provide 'path' or 'base64'")),
            };
        };

        let model = std::env::var("SOMA_VISION_MODEL")
            .unwrap_or_else(|_| "qwen2.5-vl:7b".to_string());
        let ollama_url =
            std::env::var("OLLAMA_URL").unwrap_or_else(|_| soma_common::OLLAMA_URL.to_string());

        let body = json!({
            "model": model,
            "prompt": prompt,
            "images": [image_b64],
            "stream": false,
        });

        // Use block_in_place to run async reqwest inside the synchronous capability context.
        // Requires multi-threaded tokio runtime (which soma-agent uses).
        let result = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                reqwest::Client::new()
                    .post(&ollama_url)
                    .json(&body)
                    .timeout(std::time::Duration::from_secs(90))
                    .send()
                    .await?
                    .json::<serde_json::Value>()
                    .await
            })
        });

        match result {
            Ok(json) => {
                let description = json["response"].as_str().unwrap_or("").to_string();
                CapabilityResult {
                    success: true,
                    data: json!({
                        "description": description,
                        "model": model,
                        "prompt": prompt,
                    }),
                    error: None,
                }
            }
            Err(e) => CapabilityResult {
                success: false,
                data: Value::Null,
                error: Some(CapabilityError::new(ErrorReason::NetworkError, format!("Vision model request failed: {}", e))),
            },
        }
    }
}

fn expand_tilde(path: &str) -> String {
    if path.starts_with("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return format!("{}/{}", home, &path[2..]);
        }
    }
    path.to_string()
}

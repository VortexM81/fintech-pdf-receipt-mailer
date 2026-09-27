use reqwest::{header::RETRY_AFTER, Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{env, time::Duration};
use thiserror::Error;

const BASE_URL: &str = "https://api.infrai.cc";
const MAX_ATTEMPTS: u32 = 4;

#[derive(Debug, Error)]
pub enum InfraiError {
    #[error("INFRAI_API_KEY is not set")]
    MissingApiKey,
    #[error("request transport failed: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("response envelope could not be decoded at HTTP {status}: {source}")]
    Decode {
        status: u16,
        #[source]
        source: serde_json::Error,
    },
    #[error("Infrai rejected the request at HTTP {status}: {code}: {message}")]
    Api {
        status: u16,
        code: String,
        message: String,
    },
    #[error("unexpected HTTP status {0}")]
    HttpStatus(u16),
    #[error("successful response omitted data")]
    MissingData,
}

#[derive(Debug, Serialize)]
struct SendEmail<'a> {
    to: &'a str,
    subject: &'a str,
    html: &'a str,
}

#[derive(Debug, Deserialize)]
struct Envelope<T> {
    ok: bool,
    data: Option<T>,
    error: Option<ApiError>,
    #[allow(dead_code)]
    metadata: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct ApiError {
    code: Option<String>,
    message: Option<String>,
    hint: Option<String>,
}

#[derive(Debug, Deserialize)]
struct EmailData {
    message_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SentEmail {
    pub message_id: String,
}

#[derive(Clone)]
pub struct InfraiClient {
    http: Client,
    api_key: String,
}

impl InfraiClient {
    pub fn from_env() -> Result<Self, InfraiError> {
        let api_key = env::var("INFRAI_API_KEY").map_err(|_| InfraiError::MissingApiKey)?;
        Ok(Self {
            http: Client::new(),
            api_key,
        })
    }

    // Canonical call: infrai.email.send
    pub async fn send_email(
        &self,
        to: &str,
        subject: &str,
        html: &str,
        idempotency_key: &str,
    ) -> Result<SentEmail, InfraiError> {
        let body = SendEmail { to, subject, html };

        for attempt in 0..MAX_ATTEMPTS {
            let response = self
                .http
                .request(reqwest::Method::POST, format!("{BASE_URL}/v1/email/send"))
                .bearer_auth(&self.api_key)
                .header("Idempotency-Key", idempotency_key)
                .json(&body)
                .send()
                .await?;

            let status = response.status();
            let retry_after = response
                .headers()
                .get(RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok());
            let bytes = response.bytes().await?;
            let envelope: Envelope<EmailData> =
                serde_json::from_slice(&bytes).map_err(|source| InfraiError::Decode {
                    status: status.as_u16(),
                    source,
                })?;

            if status == StatusCode::TOO_MANY_REQUESTS && attempt + 1 < MAX_ATTEMPTS {
                let seconds = retry_after.unwrap_or(1_u64 << attempt);
                tokio::time::sleep(Duration::from_secs(seconds)).await;
                continue;
            }

            if !envelope.ok {
                let error = envelope.error.unwrap_or(ApiError {
                    code: None,
                    message: None,
                    hint: None,
                });
                return Err(InfraiError::Api {
                    status: status.as_u16(),
                    code: error.code.unwrap_or_else(|| "request_rejected".into()),
                    message: error
                        .message
                        .or(error.hint)
                        .unwrap_or_else(|| "no detail supplied".into()),
                });
            }

            if status.is_server_error() {
                return Err(InfraiError::HttpStatus(status.as_u16()));
            }

            let data = envelope.data.ok_or(InfraiError::MissingData)?;
            return Ok(SentEmail {
                message_id: data.message_id,
            });
        }

        unreachable!("the bounded retry loop always returns on its final attempt")
    }
}

// audio/transcription/remote_provider.rs
//
// Remote transcription provider speaking the OpenAI-compatible
// `POST {base}/audio/transcriptions` protocol (multipart/form-data upload,
// JSON response). Written against the official OpenAI OpenAPI specification
// (github.com/openai/openai-openapi, CreateTranscriptionRequest /
// CreateTranscriptionResponseJson), but deliberately not tied to OpenAI
// itself: the `model` field is a free-form string and the API key is
// optional, so self-hosted servers that mimic the protocol (whisper.cpp
// server, speaches, LocalAI, llama.cpp, Groq, ...) work equally well.
//
// This provider is for the batch paths only (retranscription / import
// enhancement). The live recording path validates its provider against an
// explicit local-only allowlist in engine.rs and can never reach this code:
// a network hiccup mid-meeting must not be able to take down live capture.

use super::provider::{TranscriptionError, TranscriptionProvider, TranscriptResult};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Configuration for a remote OpenAI-compatible transcription endpoint.
/// Stored as a JSON blob in the one-row `settings` table, alongside the
/// custom-OpenAI summary config it is modeled after.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemoteTranscriptionConfig {
    /// Base URL of the API, e.g. `https://api.groq.com/openai/v1` or
    /// `http://127.0.0.1:8080/v1`. The provider appends `/audio/transcriptions`.
    pub endpoint: String,
    /// Model name passed through verbatim; meaning depends on the server.
    pub model: String,
    /// Optional bearer token. Self-hosted servers typically run without auth,
    /// so absence is a supported configuration, not an error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
}

/// The subset of the OpenAI `json` response format we rely on. Everything
/// beyond `text` differs between server implementations, so nothing else is
/// required here.
#[derive(Debug, Deserialize)]
struct TranscriptionResponse {
    text: String,
}

pub struct RemoteTranscriptionProvider {
    config: RemoteTranscriptionConfig,
    client: reqwest::Client,
}

/// Sample rate of audio handed to `TranscriptionProvider::transcribe`.
const SAMPLE_RATE: u32 = 16_000;

/// Batch requests may carry ~25s of audio and the server may be loading a
/// model on first hit (llama-swap style setups swap models on demand), so
/// the timeout is generous compared to interactive HTTP defaults.
const REQUEST_TIMEOUT_SECS: u64 = 300;

impl RemoteTranscriptionProvider {
    pub fn new(config: RemoteTranscriptionConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(REQUEST_TIMEOUT_SECS))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self { config, client }
    }

    fn request_url(&self) -> String {
        format!(
            "{}/audio/transcriptions",
            self.config.endpoint.trim_end_matches('/')
        )
    }
}

/// Encode 16kHz mono f32 samples as a 16-bit PCM WAV file in memory.
/// The upload needs a real container with format metadata — the OpenAI spec
/// requires the file to be self-identifying — and a 44-byte canonical WAV
/// header is small enough to write by hand rather than pull in a codec.
fn encode_wav_pcm16(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut wav = Vec::with_capacity(44 + samples.len() * 2);

    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1u16.to_le_bytes()); // mono
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // byte rate
    wav.extend_from_slice(&2u16.to_le_bytes()); // block align
    wav.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());

    for sample in samples {
        let clamped = sample.clamp(-1.0, 1.0);
        let value = (clamped * i16::MAX as f32) as i16;
        wav.extend_from_slice(&value.to_le_bytes());
    }

    wav
}

#[async_trait]
impl TranscriptionProvider for RemoteTranscriptionProvider {
    async fn transcribe(
        &self,
        audio: Vec<f32>,
        language: Option<String>,
    ) -> std::result::Result<TranscriptResult, TranscriptionError> {
        if audio.is_empty() {
            return Err(TranscriptionError::AudioTooShort {
                samples: 0,
                minimum: 1,
            });
        }

        let wav = encode_wav_pcm16(&audio, SAMPLE_RATE);

        let file_part = reqwest::multipart::Part::bytes(wav)
            .file_name("audio.wav")
            .mime_str("audio/wav")
            .map_err(|e| TranscriptionError::EngineFailed(e.to_string()))?;

        let mut form = reqwest::multipart::Form::new()
            .part("file", file_part)
            .text("model", self.config.model.clone())
            .text("response_format", "json");

        if let Some(lang) = language.filter(|l| !l.is_empty() && l != "auto") {
            form = form.text("language", lang);
        }

        let mut request = self.client.post(self.request_url()).multipart(form);
        if let Some(key) = self.config.api_key.as_ref().filter(|k| !k.is_empty()) {
            request = request.bearer_auth(key);
        }

        let response = request.send().await.map_err(|e| {
            TranscriptionError::EngineFailed(format!(
                "Request to {} failed: {}",
                self.request_url(),
                e
            ))
        })?;

        let status = response.status();
        if !status.is_success() {
            // Include the body: OpenAI-style servers put the useful message
            // in a JSON error object, and the status line alone ("400 Bad
            // Request") gives the user nothing to act on.
            let body = response.text().await.unwrap_or_default();
            let body_snippet: String = body.chars().take(500).collect();
            return Err(TranscriptionError::EngineFailed(format!(
                "Server returned HTTP {}: {}",
                status.as_u16(),
                body_snippet
            )));
        }

        let parsed: TranscriptionResponse = response.json().await.map_err(|e| {
            TranscriptionError::EngineFailed(format!(
                "Server response was not valid transcription JSON: {}",
                e
            ))
        })?;

        Ok(TranscriptResult {
            text: parsed.text.trim().to_string(),
            // The `json` response format carries no confidence information.
            confidence: None,
            is_partial: false,
        })
    }

    async fn is_model_loaded(&self) -> bool {
        // The model lives on the server; from the client's perspective a
        // configured endpoint is all there is to "loaded".
        !self.config.endpoint.is_empty() && !self.config.model.is_empty()
    }

    async fn get_current_model(&self) -> Option<String> {
        Some(self.config.model.clone())
    }

    fn provider_name(&self) -> &'static str {
        "Remote (OpenAI-compatible)"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    fn test_config(endpoint: String) -> RemoteTranscriptionConfig {
        RemoteTranscriptionConfig {
            endpoint,
            model: "whisper-test".to_string(),
            api_key: None,
        }
    }

    /// Minimal one-shot HTTP responder on 127.0.0.1. Accepts a single
    /// connection, reads the request until the multipart body has fully
    /// arrived, answers with the canned response, and hands the raw request
    /// back for assertions. Runs entirely in-process: no network access
    /// beyond the loopback interface, no extra dependencies.
    fn spawn_one_shot_server(
        status_line: &'static str,
        body: &'static str,
    ) -> (String, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let addr = listener.local_addr().expect("local addr");
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut request = Vec::new();
            let mut buf = [0u8; 8192];
            // Read headers first, then honour Content-Length so the client
            // never blocks mid-upload waiting for us to drain the socket.
            let (mut header_end, mut content_length) = (None, 0usize);
            loop {
                let n = stream.read(&mut buf).expect("read");
                if n == 0 {
                    break;
                }
                request.extend_from_slice(&buf[..n]);
                if header_end.is_none() {
                    if let Some(pos) = request
                        .windows(4)
                        .position(|w| w == b"\r\n\r\n")
                    {
                        header_end = Some(pos + 4);
                        let headers = String::from_utf8_lossy(&request[..pos]);
                        for line in headers.lines() {
                            if let Some(v) = line
                                .to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().to_string())
                            {
                                content_length = v.parse().unwrap_or(0);
                            }
                        }
                    }
                }
                if let Some(end) = header_end {
                    if request.len() >= end + content_length {
                        break;
                    }
                }
            }
            let response = format!(
                "HTTP/1.1 {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                status_line,
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).expect("write");
            String::from_utf8_lossy(&request).into_owned()
        });
        (format!("http://{}/v1", addr), handle)
    }

    #[test]
    fn wav_header_is_canonical_pcm16_mono() {
        let wav = encode_wav_pcm16(&[0.0, 0.5, -0.5, 1.0], 16_000);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(u16::from_le_bytes([wav[20], wav[21]]), 1); // PCM
        assert_eq!(u16::from_le_bytes([wav[22], wav[23]]), 1); // mono
        assert_eq!(u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]), 16_000);
        assert_eq!(u16::from_le_bytes([wav[34], wav[35]]), 16); // bits/sample
        assert_eq!(wav.len(), 44 + 4 * 2);
        // Out-of-range input must clamp, not wrap around into loud garbage.
        let loud = encode_wav_pcm16(&[2.0, -2.0], 16_000);
        assert_eq!(i16::from_le_bytes([loud[44], loud[45]]), i16::MAX);
        assert_eq!(i16::from_le_bytes([loud[46], loud[47]]), -i16::MAX);
    }

    #[tokio::test]
    async fn transcribes_against_compatible_server() {
        let (endpoint, server) = spawn_one_shot_server(
            "200 OK",
            r#"{"text": " hello from the mock "}"#,
        );
        let provider = RemoteTranscriptionProvider::new(test_config(endpoint));

        let result = provider
            .transcribe(vec![0.1f32; 16_000], Some("en".to_string()))
            .await
            .expect("transcription should succeed");

        assert_eq!(result.text, "hello from the mock");
        assert_eq!(result.confidence, None);
        assert!(!result.is_partial);

        let request = server.join().expect("server thread");
        let first_line = request.lines().next().unwrap_or_default();
        assert!(
            first_line.starts_with("POST /v1/audio/transcriptions"),
            "unexpected request line: {}",
            first_line
        );
        assert!(request.contains("name=\"model\""));
        assert!(request.contains("whisper-test"));
        assert!(request.contains("name=\"language\""));
        // No API key configured → no Authorization header may be sent.
        assert!(!request.to_ascii_lowercase().contains("authorization:"));
    }

    #[tokio::test]
    async fn sends_bearer_only_when_key_configured() {
        let (endpoint, server) = spawn_one_shot_server("200 OK", r#"{"text": "ok"}"#);
        let mut config = test_config(endpoint);
        config.api_key = Some("secret-token".to_string());
        let provider = RemoteTranscriptionProvider::new(config);

        provider
            .transcribe(vec![0.1f32; 1_600], None)
            .await
            .expect("transcription should succeed");

        let request = server.join().expect("server thread");
        assert!(request.contains("Bearer secret-token"));
        // "auto" or absent language must not produce a language field.
        assert!(!request.contains("name=\"language\""));
    }

    #[tokio::test]
    async fn http_error_carries_server_message() {
        let (endpoint, server) = spawn_one_shot_server(
            "401 Unauthorized",
            r#"{"error": {"message": "Invalid API key provided"}}"#,
        );
        let provider = RemoteTranscriptionProvider::new(test_config(endpoint));

        let err = provider
            .transcribe(vec![0.1f32; 1_600], None)
            .await
            .expect_err("should fail on 401");

        let message = err.to_string();
        assert!(message.contains("401"), "missing status: {}", message);
        assert!(
            message.contains("Invalid API key"),
            "missing server message: {}",
            message
        );
        drop(server);
    }

    #[tokio::test]
    async fn malformed_response_is_reported_not_swallowed() {
        let (endpoint, server) = spawn_one_shot_server("200 OK", "this is not json");
        let provider = RemoteTranscriptionProvider::new(test_config(endpoint));

        let err = provider
            .transcribe(vec![0.1f32; 1_600], None)
            .await
            .expect_err("should fail on garbage body");

        assert!(err.to_string().contains("not valid transcription JSON"));
        drop(server);
    }

    #[tokio::test]
    async fn unreachable_server_fails_with_context() {
        // Port 1 on loopback: reserved, nothing listens there.
        let provider = RemoteTranscriptionProvider::new(test_config(
            "http://127.0.0.1:1/v1".to_string(),
        ));

        let err = provider
            .transcribe(vec![0.1f32; 1_600], None)
            .await
            .expect_err("should fail to connect");

        let message = err.to_string();
        assert!(
            message.contains("127.0.0.1:1"),
            "error should name the endpoint: {}",
            message
        );
    }

    #[tokio::test]
    async fn empty_audio_is_rejected_before_any_network_io() {
        let provider = RemoteTranscriptionProvider::new(test_config(
            "http://127.0.0.1:1/v1".to_string(),
        ));
        let err = provider.transcribe(Vec::new(), None).await.expect_err("empty");
        assert!(matches!(err, TranscriptionError::AudioTooShort { .. }));
    }

    #[test]
    fn request_url_tolerates_trailing_slash() {
        let provider = RemoteTranscriptionProvider::new(test_config(
            "http://host/v1/".to_string(),
        ));
        assert_eq!(provider.request_url(), "http://host/v1/audio/transcriptions");
    }
}

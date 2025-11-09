//! Streaming support for LiteRT-LM

#[allow(unused_imports)]
use crate::error::LlmError as _;
use crate::error::LlmResult;
use crate::litert_wrapper::LiteRTSession;
#[allow(unused_imports)]
use std::sync::Arc as _;
use tokio::sync::mpsc;

/// Stream of tokens from LLM generation
pub struct TokenStream {
    receiver: mpsc::Receiver<String>,
}

impl TokenStream {
    pub fn new(receiver: mpsc::Receiver<String>) -> Self {
        Self { receiver }
    }

    /// Get the next token from the stream
    pub async fn next(&mut self) -> Option<String> {
        self.receiver.recv().await
    }
}

/// Generate content with streaming (optimized word-by-word)
pub async fn generate_streaming(
    conversation: &LiteRTSession,
    prompt: &str,
) -> LlmResult<TokenStream> {
    // Create a channel for streaming tokens
    let (tx, rx) = mpsc::channel(100);

    // LiteRT doesn't currently expose a streaming API in the bindings
    // Until litert_lm_session_generate_content_stream is available,
    // we use word-by-word streaming for better UX than char-by-char
    let response = conversation.send_user_message(prompt)?;

    // Stream word-by-word for smoother experience
    tokio::spawn(async move {
        let words: Vec<&str> = response.split_whitespace().collect();

        for (i, word) in words.iter().enumerate() {
            let mut token = word.to_string();
            // Add space after word (except last)
            if i < words.len() - 1 {
                token.push(' ');
            }

            if tx.send(token).await.is_err() {
                break;
            }

            // Faster streaming - 5ms per word feels natural
            tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
        }
    });

    Ok(TokenStream::new(rx))
}

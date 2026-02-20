#![deny(warnings)]

use crate::error::{Result, TransportError};
use tokio::io::{self, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, Stdin, Stdout};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StdioFraming {
    Auto,
    Newline,
    ContentLength,
}

fn trim_crlf(s: &str) -> &str {
    s.trim_end_matches(&['\r', '\n'][..])
}

fn parse_content_length_header(line: &str) -> Option<usize> {
    let line = trim_crlf(line).trim();
    let (name, value) = line.split_once(':')?;
    if !name.trim().eq_ignore_ascii_case("content-length") {
        return None;
    }
    value.trim().parse::<usize>().ok()
}

pub struct StdioTransportHandler {
    stdin: BufReader<Stdin>,
    stdout: Stdout,
    framing: StdioFraming,
}

impl Default for StdioTransportHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl StdioTransportHandler {
    pub fn new() -> Self {
        Self {
            stdin: BufReader::new(io::stdin()),
            stdout: io::stdout(),
            framing: StdioFraming::Auto,
        }
    }

    pub async fn read_message(&mut self) -> Result<String> {
        match self.framing {
            StdioFraming::Auto => self.read_message_auto().await,
            StdioFraming::Newline => self.read_message_newline().await,
            StdioFraming::ContentLength => self.read_message_content_length().await,
        }
    }

    pub async fn write_message(&mut self, message: &str) -> Result<()> {
        match self.framing {
            StdioFraming::ContentLength => self.write_message_content_length(message).await,
            StdioFraming::Auto | StdioFraming::Newline => {
                self.write_message_newline(message).await
            }
        }
    }

    async fn write_message_newline(&mut self, message: &str) -> Result<()> {
        self.stdout
            .write_all(message.as_bytes())
            .await
            .map_err(TransportError::Io)?;
        self.stdout.write_all(b"\n").await.map_err(TransportError::Io)?;
        self.stdout.flush().await.map_err(TransportError::Io)?;
        Ok(())
    }

    async fn write_message_content_length(&mut self, message: &str) -> Result<()> {
        let bytes = message.as_bytes();
        let header = format!("Content-Length: {}\r\n\r\n", bytes.len());
        self.stdout
            .write_all(header.as_bytes())
            .await
            .map_err(TransportError::Io)?;
        self.stdout.write_all(bytes).await.map_err(TransportError::Io)?;
        self.stdout.flush().await.map_err(TransportError::Io)?;
        Ok(())
    }

    async fn read_message_newline(&mut self) -> Result<String> {
        let mut line = String::new();
        let n = self.stdin.read_line(&mut line).await.map_err(TransportError::Io)?;
        if n == 0 {
            return Err(TransportError::ConnectionClosed.into());
        }
        Ok(trim_crlf(&line).to_string())
    }

    async fn read_message_auto(&mut self) -> Result<String> {
        loop {
            let mut line = String::new();
            let n = self.stdin.read_line(&mut line).await.map_err(TransportError::Io)?;
            if n == 0 {
                return Err(TransportError::ConnectionClosed.into());
            }
            let line_trimmed = trim_crlf(&line);
            if line_trimmed.trim().is_empty() {
                continue;
            }
            if parse_content_length_header(line_trimmed).is_some() {
                self.framing = StdioFraming::ContentLength;
                return self
                    .read_message_content_length_with_first_line(line_trimmed)
                    .await;
            }
            self.framing = StdioFraming::Newline;
            return Ok(line_trimmed.to_string());
        }
    }

    async fn read_message_content_length(&mut self) -> Result<String> {
        let mut first = String::new();
        let n = self.stdin.read_line(&mut first).await.map_err(TransportError::Io)?;
        if n == 0 {
            return Err(TransportError::ConnectionClosed.into());
        }
        self.read_message_content_length_with_first_line(trim_crlf(&first)).await
    }

    async fn read_message_content_length_with_first_line(
        &mut self,
        first_line: &str,
    ) -> Result<String> {
        let content_length =
            parse_content_length_header(first_line).ok_or_else(|| {
                TransportError::InvalidMessage(format!(
                    "Expected Content-Length header, got: {first_line}"
                ))
            })?;
        // Drain any remaining headers until blank line.
        loop {
            let mut line = String::new();
            let n = self.stdin.read_line(&mut line).await.map_err(TransportError::Io)?;
            if n == 0 {
                return Err(TransportError::ConnectionClosed.into());
            }
            if trim_crlf(&line).trim().is_empty() {
                break;
            }
        }
        let mut buf = vec![0u8; content_length];
        self.stdin
            .read_exact(&mut buf)
            .await
            .map_err(TransportError::Io)?;
        String::from_utf8(buf).map_err(|e| {
            TransportError::InvalidMessage(format!("Invalid UTF-8: {e}")).into()
        })
    }
}

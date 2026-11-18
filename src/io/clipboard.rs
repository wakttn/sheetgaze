use std::io::Write;

use base64::Engine;

use crate::config::ClipboardMode;
use crate::domain::error::ClipboardError;

pub trait ClipboardSink: Send {
    fn write(&mut self, text: &str) -> Result<(), ClipboardError>;
    fn read(&mut self) -> Result<String, ClipboardError>;
    fn name(&self) -> &'static str;
}

pub struct Osc52Sink {
    writer: Box<dyn Write + Send>,
    max_bytes: usize,
}

impl Osc52Sink {
    pub fn new(max_bytes: usize) -> Self {
        Self {
            writer: Box::new(std::io::stdout()),
            max_bytes,
        }
    }

    #[cfg(test)]
    pub fn with_writer(writer: Box<dyn Write + Send>, max_bytes: usize) -> Self {
        Self { writer, max_bytes }
    }
}

impl ClipboardSink for Osc52Sink {
    fn write(&mut self, text: &str) -> Result<(), ClipboardError> {
        let payload = base64::engine::general_purpose::STANDARD.encode(text.as_bytes());
        if payload.len() > self.max_bytes {
            return Err(ClipboardError::Osc52PayloadTooLarge {
                bytes: payload.len(),
                limit: self.max_bytes,
            });
        }
        let sequence = if std::env::var_os("TMUX").is_some() {
            format!("\x1bPtmux;\x1b\x1b]52;c;{payload}\x07\x1b\\")
        } else {
            format!("\x1b]52;c;{payload}\x07")
        };
        self.writer
            .write_all(sequence.as_bytes())
            .and_then(|_| self.writer.flush())
            .map_err(|error| ClipboardError::Write(error.to_string()))
    }

    fn read(&mut self) -> Result<String, ClipboardError> {
        Err(ClipboardError::ReadUnavailable)
    }

    fn name(&self) -> &'static str {
        "osc52"
    }
}

#[cfg(feature = "system-clipboard")]
pub struct SystemClipboardSink {
    clipboard: arboard::Clipboard,
}

#[cfg(feature = "system-clipboard")]
impl SystemClipboardSink {
    pub fn new() -> Result<Self, ClipboardError> {
        arboard::Clipboard::new()
            .map(|clipboard| Self { clipboard })
            .map_err(|error| ClipboardError::Write(error.to_string()))
    }
}

#[cfg(feature = "system-clipboard")]
impl ClipboardSink for SystemClipboardSink {
    fn write(&mut self, text: &str) -> Result<(), ClipboardError> {
        self.clipboard
            .set_text(text.to_string())
            .map_err(|error| ClipboardError::Write(error.to_string()))
    }

    fn read(&mut self) -> Result<String, ClipboardError> {
        self.clipboard
            .get_text()
            .map_err(|error| ClipboardError::Write(error.to_string()))
    }

    fn name(&self) -> &'static str {
        "system"
    }
}

#[cfg(not(feature = "system-clipboard"))]
pub struct SystemClipboardSink;

#[cfg(not(feature = "system-clipboard"))]
impl SystemClipboardSink {
    pub fn new() -> Result<Self, ClipboardError> {
        Err(ClipboardError::Unavailable)
    }
}

#[cfg(not(feature = "system-clipboard"))]
impl ClipboardSink for SystemClipboardSink {
    fn write(&mut self, _text: &str) -> Result<(), ClipboardError> {
        Err(ClipboardError::Unavailable)
    }

    fn read(&mut self) -> Result<String, ClipboardError> {
        Err(ClipboardError::ReadUnavailable)
    }

    fn name(&self) -> &'static str {
        "system"
    }
}

pub struct ClipboardService {
    sinks: Vec<Box<dyn ClipboardSink>>,
}

#[derive(Clone, Debug)]
pub struct CopyResult {
    pub sink: &'static str,
    pub bytes: usize,
}

#[derive(Clone, Debug)]
pub struct ClipboardRead {
    pub text: String,
    pub sink: &'static str,
}

impl ClipboardService {
    pub fn from_mode(mode: ClipboardMode, osc52_max_bytes: usize) -> Self {
        let mut sinks: Vec<Box<dyn ClipboardSink>> = Vec::new();
        match mode {
            ClipboardMode::None => {}
            ClipboardMode::Osc52 => sinks.push(Box::new(Osc52Sink::new(osc52_max_bytes))),
            ClipboardMode::System => {
                if let Ok(sink) = SystemClipboardSink::new() {
                    sinks.push(Box::new(sink));
                }
            }
            ClipboardMode::Auto => {
                let remote = std::env::var_os("SSH_CONNECTION").is_some()
                    || std::env::var_os("TMUX").is_some();
                if !remote && let Ok(sink) = SystemClipboardSink::new() {
                    sinks.push(Box::new(sink));
                }
                sinks.push(Box::new(Osc52Sink::new(osc52_max_bytes)));
            }
        }
        Self { sinks }
    }

    pub fn write(&mut self, text: &str) -> Result<CopyResult, ClipboardError> {
        if self.sinks.is_empty() {
            return Err(ClipboardError::Unavailable);
        }
        let mut last_error = None;
        for sink in &mut self.sinks {
            match sink.write(text) {
                Ok(()) => {
                    return Ok(CopyResult {
                        sink: sink.name(),
                        bytes: text.len(),
                    });
                }
                Err(error) => last_error = Some(error),
            }
        }
        Err(last_error.unwrap_or(ClipboardError::Unavailable))
    }

    pub fn read(&mut self) -> Result<ClipboardRead, ClipboardError> {
        if self.sinks.is_empty() {
            return Err(ClipboardError::ReadUnavailable);
        }
        let mut last_error = None;
        for sink in &mut self.sinks {
            match sink.read() {
                Ok(text) => {
                    return Ok(ClipboardRead {
                        text,
                        sink: sink.name(),
                    });
                }
                Err(error) => last_error = Some(error),
            }
        }
        Err(last_error.unwrap_or(ClipboardError::ReadUnavailable))
    }
}

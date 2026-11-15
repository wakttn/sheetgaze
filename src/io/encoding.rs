use std::path::Path;

use encoding_rs::{Encoding, UTF_8};

use crate::domain::error::EncodingError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncodingSource {
    Bom,
    Detected,
    Override,
}

#[derive(Clone, Copy, Debug)]
pub struct EncodingInfo {
    pub encoding: &'static Encoding,
    pub source: EncodingSource,
    pub had_errors: bool,
}

impl EncodingInfo {
    pub fn label(&self) -> &'static str {
        self.encoding.name()
    }
}

pub fn detect(sample: &[u8], override_label: Option<&str>) -> Result<EncodingInfo, EncodingError> {
    if let Some((encoding, _bom_len)) = Encoding::for_bom(sample) {
        return Ok(EncodingInfo {
            encoding,
            source: EncodingSource::Bom,
            had_errors: false,
        });
    }
    if let Some(label) = override_label {
        let encoding = Encoding::for_label(label.as_bytes())
            .ok_or_else(|| EncodingError::UnknownLabel(label.to_string()))?;
        return Ok(EncodingInfo {
            encoding,
            source: EncodingSource::Override,
            had_errors: false,
        });
    }
    let mut detector = chardetng::EncodingDetector::new(chardetng::Iso2022JpDetection::Allow);
    detector.feed(sample, true);
    let encoding = detector.guess(None, chardetng::Utf8Detection::Allow);
    if encoding == UTF_8 {
        return Ok(EncodingInfo {
            encoding,
            source: EncodingSource::Detected,
            had_errors: std::str::from_utf8(sample).is_err(),
        });
    }
    Ok(EncodingInfo {
        encoding,
        source: EncodingSource::Detected,
        had_errors: false,
    })
}

pub fn decode(bytes: &[u8], info: EncodingInfo) -> Result<(String, bool), EncodingError> {
    let (text, _encoding, had_errors) = info.encoding.decode(bytes);
    Ok((text.into_owned(), had_errors || info.had_errors))
}

pub fn read_text(
    path: &Path,
    override_label: Option<&str>,
) -> Result<(String, EncodingInfo), EncodingError> {
    let bytes = std::fs::read(path)?;
    let sample_len = bytes.len().min(64 * 1024);
    let info = detect(&bytes[..sample_len], override_label)?;
    let (text, had_errors) = decode(&bytes, info)?;
    Ok((text, EncodingInfo { had_errors, ..info }))
}

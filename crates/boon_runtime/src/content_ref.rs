use boon_plan::FiniteReal;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use crate::Value;

pub const CONTENT_DIGEST_BYTES: usize = 32;
pub const MAX_CONTENT_MEDIA_BYTES: usize = 256;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentRefError {
    diagnostic: &'static str,
}

impl ContentRefError {
    fn new(diagnostic: &'static str) -> Self {
        Self { diagnostic }
    }

    pub const fn diagnostic(&self) -> &'static str {
        self.diagnostic
    }
}

impl fmt::Display for ContentRefError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.diagnostic)
    }
}

impl std::error::Error for ContentRefError {}

/// Durable, serializable identity for immutable host-owned content.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ContentRef {
    digest: [u8; CONTENT_DIGEST_BYTES],
    size: u64,
    media: Arc<str>,
}

impl ContentRef {
    pub fn new(
        digest: [u8; CONTENT_DIGEST_BYTES],
        size: u64,
        media: impl Into<Arc<str>>,
    ) -> Result<Self, ContentRefError> {
        let media = media.into();
        validate_media(&media)?;
        Ok(Self {
            digest,
            size,
            media,
        })
    }

    pub const fn digest(&self) -> [u8; CONTENT_DIGEST_BYTES] {
        self.digest
    }

    pub const fn size(&self) -> u64 {
        self.size
    }

    pub fn media(&self) -> &str {
        &self.media
    }

    pub fn value(&self) -> Result<Value, ContentRefError> {
        let size = i64::try_from(self.size)
            .map_err(|_| ContentRefError::new("content size exceeds the Boon Number range"))?;
        let size = FiniteReal::from_i64_exact(size).map_err(|_| {
            ContentRefError::new("content size is not exactly representable as a Boon Number")
        })?;
        Ok(Value::Record(BTreeMap::from([
            (
                "digest".to_owned(),
                Value::Bytes(self.digest.to_vec().into()),
            ),
            ("size".to_owned(), Value::Number(size)),
            ("media".to_owned(), Value::Text(self.media.to_string())),
        ])))
    }

    pub fn from_value(value: &Value) -> Result<Self, ContentRefError> {
        let Value::Record(fields) = value else {
            return Err(ContentRefError::new("content reference must be a record"));
        };
        if fields.len() != 3
            || !fields.contains_key("digest")
            || !fields.contains_key("size")
            || !fields.contains_key("media")
        {
            return Err(ContentRefError::new(
                "content reference fields differ from the typed contract",
            ));
        }
        let digest = match fields.get("digest") {
            Some(Value::Bytes(digest)) => <[u8; CONTENT_DIGEST_BYTES]>::try_from(digest.as_ref())
                .map_err(|_| {
                ContentRefError::new("content digest must contain exactly 32 bytes")
            })?,
            _ => return Err(ContentRefError::new("content digest must be Bytes")),
        };
        let size = match fields.get("size") {
            Some(Value::Number(size)) => size
                .to_i64_exact()
                .ok()
                .and_then(|size| u64::try_from(size).ok())
                .ok_or_else(|| {
                    ContentRefError::new("content size must be a non-negative exact whole Number")
                })?,
            _ => return Err(ContentRefError::new("content size must be Number")),
        };
        let media = match fields.get("media") {
            Some(Value::Text(media)) => Arc::<str>::from(media.as_str()),
            _ => return Err(ContentRefError::new("content media must be Text")),
        };
        Self::new(digest, size, media)
    }
}

fn validate_media(media: &str) -> Result<(), ContentRefError> {
    if media.is_empty()
        || media.len() > MAX_CONTENT_MEDIA_BYTES
        || media.trim() != media
        || media.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(ContentRefError::new(
            "content media is empty, untrimmed, contains control bytes, or exceeds its bound",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_reference_round_trips_as_ordinary_boon_data() {
        let content = ContentRef::new([7; 32], 42, "application/octet-stream").unwrap();
        assert_eq!(
            ContentRef::from_value(&content.value().unwrap()).unwrap(),
            content
        );
        assert!(ContentRef::new([0; 32], 0, " text/plain").is_err());
    }
}

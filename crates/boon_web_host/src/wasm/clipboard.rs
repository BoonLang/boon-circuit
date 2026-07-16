use super::{js_error, window};
use crate::{BrowserClipboardCapability, WebHostError, WebHostResult};
use wasm_bindgen_futures::JsFuture;

#[derive(Clone, Debug)]
pub struct BrowserClipboardAdapter {
    capability: BrowserClipboardCapability,
}

impl BrowserClipboardAdapter {
    pub fn new(capability: BrowserClipboardCapability) -> WebHostResult<Self> {
        if capability.max_text_bytes == 0 {
            return Err(WebHostError::InvalidInput {
                field: "clipboard max_text_bytes".to_owned(),
                reason: "must be non-zero".to_owned(),
            });
        }
        Ok(Self { capability })
    }

    pub async fn read_text(&self, user_activated: bool) -> WebHostResult<String> {
        self.capability.validate_text("", user_activated)?;
        let clipboard = window()?.navigator().clipboard();
        let value = JsFuture::from(clipboard.read_text())
            .await
            .map_err(|error| js_error("read browser clipboard", error))?;
        let text = value.as_string().ok_or_else(|| {
            WebHostError::platform("read browser clipboard", "clipboard result is not text")
        })?;
        self.capability.validate_text(&text, user_activated)?;
        Ok(text)
    }

    pub async fn write_text(&self, text: &str, user_activated: bool) -> WebHostResult<()> {
        self.capability.validate_text(text, user_activated)?;
        let clipboard = window()?.navigator().clipboard();
        JsFuture::from(clipboard.write_text(text))
            .await
            .map_err(|error| js_error("write browser clipboard", error))?;
        Ok(())
    }
}

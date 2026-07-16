use super::{js_error, window};
use crate::{
    BrowserHistoryCapability, BrowserHistoryEntry, BrowserHistoryMutation, WebHostError,
    WebHostResult,
};
use js_sys::Uint8Array;
use wasm_bindgen::JsValue;

#[derive(Clone, Debug)]
pub struct BrowserHistoryAdapter {
    capability: BrowserHistoryCapability,
}

impl BrowserHistoryAdapter {
    pub fn new(capability: BrowserHistoryCapability) -> WebHostResult<Self> {
        capability.validate_entry(&BrowserHistoryEntry {
            path_query_fragment: capability.path_prefix.clone(),
            state: Vec::new(),
        })?;
        Ok(Self { capability })
    }

    pub fn current_path_query_fragment(&self) -> WebHostResult<String> {
        let location = window()?.location();
        let path = location
            .pathname()
            .map_err(|error| js_error("read location pathname", error))?;
        let query = location
            .search()
            .map_err(|error| js_error("read location query", error))?;
        let fragment = location
            .hash()
            .map_err(|error| js_error("read location fragment", error))?;
        let value = format!("{path}{query}{fragment}");
        self.capability.validate_entry(&BrowserHistoryEntry {
            path_query_fragment: value.clone(),
            state: Vec::new(),
        })?;
        Ok(value)
    }

    pub fn mutate(
        &self,
        mutation: BrowserHistoryMutation,
        entry: &BrowserHistoryEntry,
    ) -> WebHostResult<()> {
        self.capability.validate_entry(entry)?;
        let history = window()?
            .history()
            .map_err(|error| js_error("access History", error))?;
        let state = Uint8Array::from(entry.state.as_slice());
        let state: JsValue = state.into();
        match mutation {
            BrowserHistoryMutation::Push => {
                history.push_state_with_url(&state, "", Some(&entry.path_query_fragment))
            }
            BrowserHistoryMutation::Replace => {
                history.replace_state_with_url(&state, "", Some(&entry.path_query_fragment))
            }
        }
        .map_err(|error| js_error("update same-origin History", error))
    }

    pub fn back(&self) -> WebHostResult<()> {
        window()?
            .history()
            .map_err(|error| js_error("access History", error))?
            .back()
            .map_err(|error| js_error("History.back", error))
    }

    pub fn forward(&self) -> WebHostResult<()> {
        window()?
            .history()
            .map_err(|error| js_error("access History", error))?
            .forward()
            .map_err(|error| js_error("History.forward", error))
    }

    pub fn go(&self, delta: i32) -> WebHostResult<()> {
        if !(-100..=100).contains(&delta) {
            return Err(WebHostError::InvalidInput {
                field: "history delta".to_owned(),
                reason: "must be within -100..=100".to_owned(),
            });
        }
        window()?
            .history()
            .map_err(|error| js_error("access History", error))?
            .go_with_delta(delta)
            .map_err(|error| js_error("History.go", error))
    }
}

//! GitHub Copilot desktop app as a *client*: it adds OwO AI Gateway under Settings → Model providers
//! as an OpenAI-compatible provider and syncs models from `GET /v1/models`. The app keeps
//! that setting in its own store, so OwO AI Gateway prints the values instead of editing files.
//! (GitHub Copilot as an upstream provider is a separate concern.)

use crate::{AppModel, Target};

pub const CLIENT_ID: &str = "copilot_app";

pub struct Instructions {
    pub base_url: String,
    /// `None` when the gateway needs no key (leave the field empty).
    pub api_key: Option<String>,
    pub model_ids: Vec<String>,
}

pub fn instructions(target: &Target, models: &[AppModel]) -> Instructions {
    Instructions { base_url: target.v1(), api_key: target.token.clone(), model_ids: models.iter().map(|m| m.id.clone()).collect() }
}

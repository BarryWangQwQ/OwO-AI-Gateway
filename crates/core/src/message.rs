use serde::{Deserialize, Serialize};

use crate::content::ContentBlock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    User,
    Assistant,
    /// A positional instruction that appeared mid-conversation (OpenAI `system`/`developer`
    /// messages after the first user turn). Leading instructions go to
    /// [`crate::ModelRequest::system`] instead; encoders for protocols without positional
    /// instructions must convert these explicitly rather than drop them.
    System,
}

/// A conversation turn. Tool results travel as [`ContentBlock::ToolResult`] inside
/// [`Role::User`] messages; protocols with a dedicated tool role split them on encode.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: Vec<ContentBlock>,
}

impl Message {
    pub fn new(role: Role, content: Vec<ContentBlock>) -> Self {
        Self { role, content }
    }

    pub fn user_text(text: impl Into<String>) -> Self {
        Self::new(Role::User, vec![ContentBlock::text(text)])
    }

    pub fn assistant_text(text: impl Into<String>) -> Self {
        Self::new(Role::Assistant, vec![ContentBlock::text(text)])
    }
}

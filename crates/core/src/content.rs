use serde::{Deserialize, Serialize};

/// One unit of message content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text { text: String },
    Image(ImageInput),
    Reasoning(ReasoningBlock),
    ToolCall(ToolCall),
    ToolResult(ToolResult),
    File(FileInput),
}

impl ContentBlock {
    pub fn text(text: impl Into<String>) -> Self {
        ContentBlock::Text { text: text.into() }
    }

    pub fn as_text(&self) -> Option<&str> {
        match self {
            ContentBlock::Text { text } => Some(text),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImageInput {
    pub source: ImageSource,
    /// Client-requested detail level (`low`, `high`, `auto`), when the inbound protocol has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ImageSource {
    Url { url: String },
    Base64 { media_type: String, data: String },
}

impl ImageSource {
    /// Parses a `data:<media>;base64,<data>` URL into [`ImageSource::Base64`], otherwise keeps the URL.
    pub fn from_url(url: &str) -> Self {
        if let Some(rest) = url.strip_prefix("data:") {
            if let Some((meta, data)) = rest.split_once(',') {
                if let Some(media_type) = meta.strip_suffix(";base64") {
                    return ImageSource::Base64 {
                        media_type: media_type.to_string(),
                        data: data.to_string(),
                    };
                }
            }
        }
        ImageSource::Url { url: url.to_string() }
    }

    /// Renders the source as a URL, using a data URL for inline bytes.
    pub fn to_url(&self) -> String {
        match self {
            ImageSource::Url { url } => url.clone(),
            ImageSource::Base64 { media_type, data } => format!("data:{media_type};base64,{data}"),
        }
    }
}

/// Model reasoning. Opaque replay material (signatures, encrypted content) is kept
/// so a later turn can hand it back to the provider that produced it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ReasoningBlock {
    #[serde(default)]
    pub text: String,
    /// Summary text, for protocols that separate summaries from raw reasoning.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encrypted_content: Option<String>,
    /// Provider-side item id, when the upstream assigned one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallKind {
    /// JSON-arguments function call.
    #[default]
    Function,
    /// Free-form input tool (OpenAI Responses `custom` tools such as Codex `apply_patch`).
    Custom,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// Raw arguments exactly as produced: a JSON document for [`ToolCallKind::Function`],
    /// free-form text for [`ToolCallKind::Custom`]. Never re-serialized, so no formatting is lost.
    pub arguments: String,
    #[serde(default)]
    pub kind: ToolCallKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolResult {
    pub call_id: String,
    pub content: Vec<ToolResultContent>,
    #[serde(default)]
    pub is_error: bool,
    #[serde(default)]
    pub kind: ToolCallKind,
}

impl ToolResult {
    /// Concatenated text of all text parts.
    pub fn text(&self) -> String {
        let mut out = String::new();
        for part in &self.content {
            if let ToolResultContent::Text { text } = part {
                out.push_str(text);
            }
        }
        out
    }

    pub fn has_images(&self) -> bool {
        self.content.iter().any(|c| matches!(c, ToolResultContent::Image(_)))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolResultContent {
    Text { text: String },
    Image(ImageInput),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
    pub source: FileSource,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FileSource {
    Base64 { data: String },
    Url { url: String },
    /// Provider-hosted file reference; only routable back to the provider that issued it.
    FileId { id: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_url_round_trip() {
        let src = ImageSource::from_url("data:image/png;base64,AAAA");
        assert_eq!(
            src,
            ImageSource::Base64 { media_type: "image/png".into(), data: "AAAA".into() }
        );
        assert_eq!(src.to_url(), "data:image/png;base64,AAAA");
    }

    #[test]
    fn plain_url_is_kept() {
        let src = ImageSource::from_url("https://example.com/a.png");
        assert_eq!(src, ImageSource::Url { url: "https://example.com/a.png".into() });
    }
}

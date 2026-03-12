//! Attachments - Image and document handling

use serde::{Deserialize, Serialize};

/// Attachment type
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum AttachmentType {
    Image,
    Document,
    Code,
    Data,
}

/// File attachment
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Attachment {
    pub name: String,
    pub mime_type: String,
    pub attachment_type: AttachmentType,
    pub data: Vec<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_content: Option<String>,
}

impl Attachment {
    /// Create image attachment from bytes
    pub fn image(name: impl Into<String>, data: Vec<u8>) -> Self {
        Self {
            name: name.into(),
            mime_type: "image/png".to_string(),
            attachment_type: AttachmentType::Image,
            data,
            text_content: None,
        }
    }

    /// Create document attachment
    pub fn document(name: impl Into<String>, mime_type: impl Into<String>, data: Vec<u8>) -> Self {
        Self {
            name: name.into(),
            mime_type: mime_type.into(),
            attachment_type: AttachmentType::Document,
            data,
            text_content: None,
        }
    }

    /// Create code attachment
    pub fn code(name: impl Into<String>, content: impl Into<String>) -> Self {
        let content = content.into();
        Self {
            name: name.into(),
            mime_type: "text/plain".to_string(),
            attachment_type: AttachmentType::Code,
            data: content.clone().into_bytes(),
            text_content: Some(content),
        }
    }

    /// Create from file path
    pub async fn from_path(path: impl AsRef<std::path::Path>) -> Result<Self, std::io::Error> {
        let path = path.as_ref();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        let data = tokio::fs::read(path).await?;

        let mime_type = match path.extension() {
            Some(ext) => match ext.to_str() {
                Some("png") => "image/png",
                Some("jpg") | Some("jpeg") => "image/jpeg",
                Some("gif") => "image/gif",
                Some("webp") => "image/webp",
                Some("pdf") => "application/pdf",
                Some("txt") => "text/plain",
                Some("md") => "text/markdown",
                Some("rs") => "text/rust",
                Some("js") => "text/javascript",
                Some("py") => "text/python",
                _ => "application/octet-stream",
            },
            None => "application/octet-stream",
        }
        .to_string();

        let attachment_type = if mime_type.starts_with("image/") {
            AttachmentType::Image
        } else if mime_type.starts_with("text/")
            || path
                .extension()
                .map(|e| e == "rs" || e == "js" || e == "py")
                .unwrap_or(false)
        {
            AttachmentType::Code
        } else {
            AttachmentType::Document
        };

        let text_content = if attachment_type == AttachmentType::Code || mime_type == "text/plain" {
            String::from_utf8(data.clone()).ok()
        } else {
            None
        };

        Ok(Self {
            name,
            mime_type,
            attachment_type,
            data,
            text_content,
        })
    }

    /// Get size in bytes
    pub fn size(&self) -> usize {
        self.data.len()
    }

    /// Format for LLM context
    pub fn to_context_string(&self) -> String {
        match &self.text_content {
            Some(text) => format!("```{}\n{}\n```", self.name, text),
            None => format!("[Attachment: {} ({} bytes)]", self.name, self.size()),
        }
    }
}

/// Attachment builder
pub struct AttachmentBuilder {
    name: String,
    mime_type: String,
    attachment_type: AttachmentType,
    data: Vec<u8>,
    text_content: Option<String>,
}

impl AttachmentBuilder {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            mime_type: "application/octet-stream".to_string(),
            attachment_type: AttachmentType::Data,
            data: Vec::new(),
            text_content: None,
        }
    }

    pub fn mime_type(mut self, mime: impl Into<String>) -> Self {
        self.mime_type = mime.into();
        self
    }

    pub fn attachment_type(mut self, att_type: AttachmentType) -> Self {
        self.attachment_type = att_type;
        self
    }

    pub fn data(mut self, data: Vec<u8>) -> Self {
        self.data = data;
        self
    }

    pub fn text_content(mut self, text: impl Into<String>) -> Self {
        self.text_content = Some(text.into());
        self
    }

    pub fn build(self) -> Attachment {
        Attachment {
            name: self.name,
            mime_type: self.mime_type,
            attachment_type: self.attachment_type,
            data: self.data,
            text_content: self.text_content,
        }
    }
}

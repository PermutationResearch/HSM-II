//! Smart response generation.

use super::{ConversationThread, Email};

/// Response generator using local LLM
pub struct ResponseGenerator {
    templates: Vec<ResponseTemplate>,
}

impl ResponseGenerator {
    pub fn new() -> Self {
        let templates = vec![
            ResponseTemplate {
                name: "acknowledge".to_string(),
                pattern: "acknowledge receipt".to_string(),
                template: "Thank you for your email. I have received your message and will review it shortly.".to_string(),
                tone: Tone::Professional,
            },
            ResponseTemplate {
                name: "meeting_accept".to_string(),
                pattern: "meeting invite".to_string(),
                template: "Thank you for the meeting invitation. I accept and look forward to our discussion.".to_string(),
                tone: Tone::Professional,
            },
            ResponseTemplate {
                name: "question_followup".to_string(),
                pattern: "question".to_string(),
                template: "Thank you for your question. Let me look into this and get back to you with a detailed response.".to_string(),
                tone: Tone::Helpful,
            },
            ResponseTemplate {
                name: "delay".to_string(),
                pattern: "delay".to_string(),
                template: "I apologize for the delayed response. Thank you for your patience.".to_string(),
                tone: Tone::Apologetic,
            },
        ];

        Self { templates }
    }

    /// Generate a response to an email
    pub async fn generate_response(
        &self,
        email: &Email,
        thread: Option<&ConversationThread>,
    ) -> anyhow::Result<String> {
        // Select appropriate template
        let template = self.select_template(email);

        // Customize based on context
        let response = self.customize_template(template, email, thread);

        Ok(response)
    }

    fn select_template(&self, email: &Email) -> &ResponseTemplate {
        let content = format!("{} {}", email.subject, email.body).to_lowercase();

        self.templates
            .iter()
            .find(|t| content.contains(&t.pattern))
            .unwrap_or(&self.templates[0])
    }

    fn customize_template(
        &self,
        template: &ResponseTemplate,
        email: &Email,
        _thread: Option<&ConversationThread>,
    ) -> String {
        let mut response = template.template.clone();

        // Add greeting
        let greeting = match template.tone {
            Tone::Professional => format!("Dear {},\n\n", extract_name(&email.from)),
            Tone::Casual => format!("Hi {},\n\n", extract_name(&email.from)),
            Tone::Helpful => format!("Hello {},\n\n", extract_name(&email.from)),
            Tone::Apologetic => format!("Dear {},\n\n", extract_name(&email.from)),
        };

        response = format!("{}{}", greeting, response);

        // Add sign-off
        let signoff = match template.tone {
            Tone::Professional => "\n\nBest regards,\nAI Assistant",
            Tone::Casual => "\n\nCheers,\nAI",
            Tone::Helpful => "\n\nLet me know if you need anything else,\nAI Assistant",
            Tone::Apologetic => "\n\nThank you for your understanding,\nAI Assistant",
        };

        response.push_str(signoff);

        response
    }

    /// Generate a quick response
    pub async fn quick_reply(&self, email: &Email, reply_type: QuickReplyType) -> String {
        match reply_type {
            QuickReplyType::Acknowledge => {
                format!(
                    "Hi {},\n\nThanks for reaching out. I've received your message and will get back to you soon.\n\nBest",
                    extract_name(&email.from)
                )
            }
            QuickReplyType::Accept => {
                format!(
                    "Hi {},\n\nThat sounds good to me. Looking forward to it!\n\nBest",
                    extract_name(&email.from)
                )
            }
            QuickReplyType::Decline => {
                format!(
                    "Hi {},\n\nThanks for the invitation, but I won't be able to make it this time.\n\nBest",
                    extract_name(&email.from)
                )
            }
            QuickReplyType::FollowUp => {
                format!(
                    "Hi {},\n\nJust following up on my previous message. Let me know if you have any updates.\n\nBest",
                    extract_name(&email.from)
                )
            }
        }
    }
}

impl Default for ResponseGenerator {
    fn default() -> Self {
        Self::new()
    }
}

/// Response template
#[derive(Clone, Debug)]
pub struct ResponseTemplate {
    pub name: String,
    pub pattern: String,
    pub template: String,
    pub tone: Tone,
}

/// Response tone
#[derive(Clone, Copy, Debug)]
pub enum Tone {
    Professional,
    Casual,
    Helpful,
    Apologetic,
}

/// Quick reply types
#[derive(Clone, Copy, Debug)]
pub enum QuickReplyType {
    Acknowledge,
    Accept,
    Decline,
    FollowUp,
}

/// Extract name from email address
fn extract_name(from: &str) -> String {
    if let Some(start) = from.find('<') {
        from[..start].trim().to_string()
    } else {
        from.split('@').next().unwrap_or("there").to_string()
    }
}

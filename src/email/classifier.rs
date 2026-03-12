//! Email classification using local LLM.

use super::Email;

/// Email classifier using semantic analysis
pub struct EmailClassifier {
    // In production, would use FrankenTorch for classification
}

impl EmailClassifier {
    pub fn new() -> Self {
        Self {}
    }

    /// Classify an email
    pub async fn classify(&self, email: &Email) -> Classification {
        // Simple rule-based classification for now
        // In production, would use LLM-based classification

        let subject_lower = email.subject.to_lowercase();
        let body_lower = email.body.to_lowercase();

        // Check for spam indicators
        if self.is_spam(&subject_lower, &body_lower) {
            return Classification {
                category: Category::Spam,
                priority: Priority::Low,
                needs_response: false,
                confidence: 0.9,
            };
        }

        // Check for newsletter
        if subject_lower.contains("newsletter")
            || subject_lower.contains("digest")
            || subject_lower.contains("unsubscribe")
        {
            return Classification {
                category: Category::Newsletter,
                priority: Priority::Low,
                needs_response: false,
                confidence: 0.85,
            };
        }

        // Check for social
        if subject_lower.contains("linkedin")
            || subject_lower.contains("twitter")
            || subject_lower.contains("facebook")
            || subject_lower.contains("invitation")
        {
            return Classification {
                category: Category::Social,
                priority: Priority::Low,
                needs_response: false,
                confidence: 0.8,
            };
        }

        // Check for notifications
        if subject_lower.contains("notification")
            || subject_lower.contains("alert")
            || subject_lower.contains("update")
        {
            return Classification {
                category: Category::Notification,
                priority: Priority::Medium,
                needs_response: false,
                confidence: 0.75,
            };
        }

        // Check if needs response
        let needs_response = subject_lower.contains("question")
            || subject_lower.contains("help")
            || subject_lower.contains("request")
            || body_lower.contains("?")
            || body_lower.contains("please")
            || body_lower.contains("could you");

        // Determine priority
        let priority = if subject_lower.contains("urgent")
            || subject_lower.contains("asap")
            || subject_lower.contains("deadline")
        {
            Priority::Critical
        } else if needs_response {
            Priority::High
        } else {
            Priority::Medium
        };

        Classification {
            category: Category::Important,
            priority,
            needs_response,
            confidence: 0.7,
        }
    }

    fn is_spam(&self, subject: &str, body: &str) -> bool {
        let spam_keywords = [
            "viagra",
            "lottery",
            "winner",
            "prize",
            "free money",
            "click here",
            "limited time",
            "act now",
            "congratulations",
            "you won",
            "cash bonus",
            "million dollars",
        ];

        spam_keywords
            .iter()
            .any(|kw| subject.contains(kw) || body.contains(kw))
    }
}

impl Default for EmailClassifier {
    fn default() -> Self {
        Self::new()
    }
}

/// Email category
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    Spam,
    Newsletter,
    Social,
    Notification,
    Important,
}

/// Email priority
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Low = 0,
    Medium = 1,
    High = 2,
    Critical = 3,
}

/// Classification result
#[derive(Clone, Debug)]
pub struct Classification {
    pub category: Category,
    pub priority: Priority,
    pub needs_response: bool,
    pub confidence: f64,
}

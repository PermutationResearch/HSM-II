//! Cheap L0/L1-style summaries when callers omit `summary_l0` / `summary_l1`.

/// First “sentence-ish” fragment up to `max_chars` (aligned with `memory.rs` spirit).
fn clip_first_sentence_ish(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let first_sentence_end = trimmed
        .find(|c: char| c == '.' || c == '!' || c == '?')
        .map(|i| i + 1)
        .unwrap_or(trimmed.len());
    let end = first_sentence_end.min(max_chars).min(trimmed.len());
    let mut s = trimmed[..end].to_string();
    if end < trimmed.len() && !s.ends_with('.') && !s.ends_with('!') && !s.ends_with('?') {
        s.push('…');
    }
    s
}

fn first_n_sentences_ish(text: &str, max_sentences: usize, max_chars: usize) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let chars: Vec<char> = trimmed.chars().collect();
    let mut sentences = Vec::new();
    let mut start = 0usize;
    for (i, &c) in chars.iter().enumerate() {
        if (c == '.' || c == '!' || c == '?') && i + 1 < chars.len() && chars[i + 1].is_whitespace()
        {
            let sentence: String = chars[start..=i].iter().collect();
            sentences.push(sentence);
            start = i + 1;
            if sentences.len() >= max_sentences {
                break;
            }
        }
    }
    if sentences.is_empty() {
        let end = trimmed.len().min(max_chars);
        let mut s = trimmed[..end].to_string();
        if end < trimmed.len() {
            s.push('…');
        }
        return s;
    }
    let mut overview = sentences.join(" ").trim().to_string();
    if overview.len() > max_chars {
        overview.truncate(max_chars);
        overview.push('…');
    }
    overview
}

/// Returns `(summary_l0, summary_l1)` suitable for DB storage.
pub fn derive_summary_l0_l1(title: &str, body: &str) -> (Option<String>, Option<String>) {
    let combined = format!("{} {}", title.trim(), body.trim());
    let t = combined.trim();
    if t.is_empty() {
        return (None, None);
    }
    let l0 = clip_first_sentence_ish(t, 80);
    let l1 = first_n_sentences_ish(t, 3, 500);
    let l0 = (!l0.is_empty()).then_some(l0);
    let l1 = (!l1.is_empty()).then_some(l1);
    (l0, l1)
}

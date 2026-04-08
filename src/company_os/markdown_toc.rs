//! Heading outline from company `context_markdown` for bootstrap-style LLM context.

/// Extract `#` / `##` / `###` lines (trimmed) as a short bullet TOC.
pub fn heading_outline(markdown: &str, max_lines: usize) -> String {
    let mut out = Vec::new();
    for line in markdown.lines() {
        let t = line.trim();
        if t.starts_with("### ") {
            out.push(format!("- {}", t.trim_start_matches('#').trim()));
        } else if t.starts_with("## ") {
            out.push(format!("- {}", t.trim_start_matches('#').trim()));
        } else if t.starts_with("# ") && !t.starts_with("##") {
            out.push(format!("- {}", t.trim_start_matches('#').trim()));
        }
        if out.len() >= max_lines {
            break;
        }
    }
    if out.is_empty() {
        return String::new();
    }
    out.join("\n")
}

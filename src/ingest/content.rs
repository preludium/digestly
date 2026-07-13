//! Content handling (prompt.md §5): ammonia sanitization (+ relative→absolute URL rewriting),
//! plain-text extraction for FTS/AI, lead-image discovery, and reading-time estimation.

use ammonia::{Builder, Url, UrlRelative};
use scraper::{Html, Selector};

/// Average adult reading speed (words per minute) used for `reading_time_secs`.
const WORDS_PER_MINUTE: f64 = 200.0;

/// Sanitize feed HTML and rewrite relative URLs to absolute against `base` (the item link).
/// Strips scripts, event handlers, and `javascript:` URLs (prompt.md §11 "Security" XSS).
pub fn sanitize_html(raw: &str, base: Option<&str>, cap: usize) -> String {
    let mut builder = Builder::default();
    if let Some(base_url) = base.and_then(|b| Url::parse(b).ok()) {
        builder.url_relative(UrlRelative::RewriteWithBase(base_url));
    }
    let mut cleaned = builder.clean(raw).to_string();
    if cleaned.chars().count() > cap {
        cleaned = cleaned.chars().take(cap).collect();
    }
    cleaned
}

/// Extract readable plain text from (already sanitized) HTML for FTS + AI + reading time.
pub fn to_text(html: &str, cap: usize) -> String {
    let fragment = Html::parse_fragment(html);
    let mut text = fragment.root_element().text().collect::<Vec<_>>().join(" ");
    text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() > cap {
        text = text.chars().take(cap).collect();
    }
    text
}

/// First `<img>` src found in HTML, resolved to absolute (HTML should already be rewritten).
pub fn first_image(html: &str) -> Option<String> {
    let fragment = Html::parse_fragment(html);
    let sel = Selector::parse("img").ok()?;
    fragment
        .select(&sel)
        .find_map(|el| el.value().attr("src").map(|s| s.to_string()))
        .filter(|s| s.starts_with("http"))
}

/// Reading time in seconds from plain text (~200 wpm), floored at a sensible minimum for
/// non-empty content so a card never shows "0 min".
pub fn reading_time_secs(text: &str) -> i64 {
    let words = text.split_whitespace().count();
    if words == 0 {
        return 0;
    }
    let secs = (words as f64 / WORDS_PER_MINUTE * 60.0).round() as i64;
    secs.max(30)
}

/// Best-effort readability extraction of an article's main content (prompt.md §5 full-text
/// toggle). Returns sanitized HTML of the densest content block, or `None` to fall back to the
/// feed's own content. Never gates ingestion - callers treat failure as "use feed content".
pub fn extract_readable(page_html: &str, base: &str, cap: usize) -> Option<String> {
    let doc = Html::parse_document(page_html);
    // Prefer semantic containers, then the block with the most paragraph text.
    for selector in ["article", "main", "[role=main]"] {
        if let Ok(sel) = Selector::parse(selector) {
            if let Some(el) = doc.select(&sel).next() {
                let html = el.inner_html();
                let text_len = to_text(&html, cap).len();
                if text_len > 200 {
                    return Some(sanitize_html(&html, Some(base), cap));
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_xss_payloads() {
        let dirty = r#"<p>hi</p><script>alert('xss')</script><a href="javascript:evil()">x</a><img src=x onerror=alert(1)>"#;
        let clean = sanitize_html(dirty, None, 10_000);
        assert!(!clean.contains("<script"));
        assert!(!clean.to_lowercase().contains("javascript:"));
        assert!(!clean.to_lowercase().contains("onerror"));
        assert!(clean.contains("hi"));
    }

    #[test]
    fn rewrites_relative_urls_to_absolute() {
        let html = r#"<a href="/page">link</a>"#;
        let clean = sanitize_html(html, Some("https://example.com/blog/post"), 10_000);
        assert!(clean.contains("https://example.com/page"));
    }

    #[test]
    fn extracts_plain_text() {
        assert_eq!(to_text("<p>Hello <b>world</b></p>", 10_000), "Hello world");
    }

    #[test]
    fn reading_time_scales_with_words() {
        let text = "word ".repeat(200);
        assert_eq!(reading_time_secs(&text), 60);
        assert_eq!(reading_time_secs(""), 0);
        assert_eq!(reading_time_secs("short"), 30); // floored
    }

    #[test]
    fn finds_first_image() {
        let html = r#"<p>x</p><img src="https://ex.com/a.png"><img src="https://ex.com/b.png">"#;
        assert_eq!(first_image(html), Some("https://ex.com/a.png".to_string()));
    }
}

//! OPML import/export (prompt.md §9.5, §9.7, §10, §11). Round-trip must be lossless: the feeds and
//! their category grouping survive export → import. Parsing tolerates the common shapes (feeds
//! nested under category outlines, or a flat list) and defaults a missing category to `Other`.
//!
//! We parse with `roxmltree` (a real XML tree). OPML `<outline>` is a non-void element, so an
//! HTML5 parser would mishandle self-closing feed outlines and their nesting; a proper XML parser
//! preserves both the tree shape and the case-sensitive `xmlUrl`/`htmlUrl` attribute names.

use crate::ingest::url_util;

/// One feed entry parsed from (or destined for) an OPML file.
#[derive(Debug, Clone, PartialEq)]
pub struct OpmlFeed {
    pub title: Option<String>,
    pub feed_url: String,
    pub html_url: Option<String>,
    pub kind: String,
    /// Category name from the parent outline; `None` → caller defaults to `Other`.
    pub category: Option<String>,
}

/// Parse OPML text into feed entries. Skips outlines without an `xmlUrl` (those are folders).
pub fn parse(xml: &str) -> Vec<OpmlFeed> {
    let Ok(doc) = roxmltree::Document::parse(xml) else { return Vec::new() };

    let mut out = Vec::new();
    for node in doc.descendants().filter(|n| n.has_tag_name("outline")) {
        let Some(raw_url) = attr(node, "xmlUrl").or_else(|| attr(node, "xmlurl")) else { continue };
        let feed_url = url_util::normalize_url(&raw_url).unwrap_or(raw_url);
        let title = attr(node, "title").or_else(|| attr(node, "text"));
        let html_url = attr(node, "htmlUrl").or_else(|| attr(node, "htmlurl"));
        let kind = infer_kind(&feed_url, node.attribute("type").unwrap_or_default());
        let category = parent_category(node);
        out.push(OpmlFeed { title, feed_url, html_url, kind, category });
    }
    out
}

/// Build an OPML document from feeds grouped by category name (each `(category, feeds)`).
pub fn build(owner: &str, groups: &[(String, Vec<OpmlFeed>)]) -> String {
    let mut s = String::new();
    s.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    s.push_str("<opml version=\"2.0\">\n  <head>\n");
    s.push_str(&format!("    <title>Digestly subscriptions — {}</title>\n", esc(owner)));
    s.push_str("  </head>\n  <body>\n");
    for (category, feeds) in groups {
        s.push_str(&format!("    <outline text=\"{0}\" title=\"{0}\">\n", esc(category)));
        for f in feeds {
            let title = esc(f.title.as_deref().unwrap_or(&f.feed_url));
            s.push_str(&format!(
                "      <outline type=\"{}\" text=\"{}\" title=\"{}\" xmlUrl=\"{}\"",
                esc(&f.kind),
                title,
                title,
                esc(&f.feed_url),
            ));
            if let Some(h) = &f.html_url {
                s.push_str(&format!(" htmlUrl=\"{}\"", esc(h)));
            }
            s.push_str("/>\n");
        }
        s.push_str("    </outline>\n");
    }
    s.push_str("  </body>\n</opml>\n");
    s
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn attr(node: roxmltree::Node, name: &str) -> Option<String> {
    node.attribute(name).map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// The nearest ancestor `<outline>` that is a folder (no `xmlUrl`) supplies the category name.
fn parent_category(node: roxmltree::Node) -> Option<String> {
    let parent = node.parent_element()?;
    if !parent.has_tag_name("outline") {
        return None;
    }
    if parent.attribute("xmlUrl").or_else(|| parent.attribute("xmlurl")).is_some() {
        return None; // parent is itself a feed, not a folder
    }
    attr(parent, "text").or_else(|| attr(parent, "title"))
}

/// Infer the feed kind from the URL and the OPML `type` attribute (best-effort).
fn infer_kind(url: &str, ty: &str) -> String {
    let u = url.to_ascii_lowercase();
    if u.contains("youtube.com/feeds/videos.xml") || u.contains("youtube.com/channel") {
        return "youtube".into();
    }
    if u.contains("reddit.com/r/") {
        return "reddit".into();
    }
    match ty.to_ascii_lowercase().as_str() {
        "atom" => "atom".into(),
        "json" | "jsonfeed" => "jsonfeed".into(),
        _ if u.ends_with(".json") => "jsonfeed".into(),
        _ => "rss".into(),
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0"?>
<opml version="2.0">
  <head><title>Test</title></head>
  <body>
    <outline text="Software Engineering" title="Software Engineering">
      <outline type="rss" text="Hacker News" title="Hacker News" xmlUrl="https://news.ycombinator.com/rss" htmlUrl="https://news.ycombinator.com"/>
    </outline>
    <outline text="AI">
      <outline type="rss" text="r/ML" xmlUrl="https://www.reddit.com/r/MachineLearning/.rss"/>
    </outline>
    <outline type="rss" text="Flat Feed" xmlUrl="https://example.com/feed.xml"/>
  </body>
</opml>"#;

    #[test]
    fn parses_nested_and_flat_outlines() {
        let feeds = parse(SAMPLE);
        assert_eq!(feeds.len(), 3);

        let hn = &feeds[0];
        assert_eq!(hn.feed_url, "https://news.ycombinator.com/rss");
        assert_eq!(hn.title.as_deref(), Some("Hacker News"));
        assert_eq!(hn.category.as_deref(), Some("Software Engineering"));
        assert_eq!(hn.html_url.as_deref(), Some("https://news.ycombinator.com"));

        let reddit = &feeds[1];
        assert_eq!(reddit.kind, "reddit", "reddit URL infers reddit kind");
        assert_eq!(reddit.category.as_deref(), Some("AI"));

        let flat = &feeds[2];
        assert_eq!(flat.category, None, "top-level feed has no category folder");
    }

    #[test]
    fn export_then_import_round_trips_losslessly() {
        let groups = vec![
            (
                "Software Engineering".to_string(),
                vec![OpmlFeed {
                    title: Some("Hacker News".into()),
                    feed_url: "https://news.ycombinator.com/rss".into(),
                    html_url: Some("https://news.ycombinator.com".into()),
                    kind: "rss".into(),
                    category: Some("Software Engineering".into()),
                }],
            ),
            (
                "AI".to_string(),
                vec![OpmlFeed {
                    title: Some("r/ML".into()),
                    feed_url: "https://www.reddit.com/r/machinelearning/.rss".into(),
                    html_url: None,
                    kind: "reddit".into(),
                    category: Some("AI".into()),
                }],
            ),
        ];
        let xml = build("alice", &groups);
        let parsed = parse(&xml);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].feed_url, "https://news.ycombinator.com/rss");
        assert_eq!(parsed[0].category.as_deref(), Some("Software Engineering"));
        assert_eq!(parsed[0].title.as_deref(), Some("Hacker News"));
        assert_eq!(parsed[1].category.as_deref(), Some("AI"));
        assert_eq!(parsed[1].kind, "reddit");
    }

    #[test]
    fn escaping_handles_ampersands_and_quotes() {
        let groups = vec![(
            "News & \"Views\"".to_string(),
            vec![OpmlFeed {
                title: Some("A & B".into()),
                feed_url: "https://ex.com/feed?a=1&b=2".into(),
                html_url: None,
                kind: "rss".into(),
                category: None,
            }],
        )];
        let xml = build("me", &groups);
        assert!(xml.contains("&amp;"));
        let parsed = parse(&xml);
        assert_eq!(parsed[0].feed_url, "https://ex.com/feed?a=1&b=2");
        assert_eq!(parsed[0].category.as_deref(), Some("News & \"Views\""));
    }
}

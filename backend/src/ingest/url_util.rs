//! URL normalization, dedup keys, and the SSRF guard (prompt.md §11).
//!
//! Normalization collapses the http/https + trailing-slash duplicates that the spec calls out,
//! so a feed added twice under cosmetic variants dedupes to one row.

use std::net::IpAddr;

use url::Url;

/// Canonicalize a URL for storage/comparison: lowercase scheme+host, drop the fragment and any
/// default port, and strip a trailing slash (except the bare root). Returns `None` if it isn't a
/// parseable absolute http(s) URL.
pub fn normalize_url(raw: &str) -> Option<String> {
    let mut u = Url::parse(raw.trim()).ok()?;
    if !matches!(u.scheme(), "http" | "https") {
        return None;
    }
    u.set_fragment(None);
    // Lowercase host (Url already lowercases scheme). host_str is already lowercased by url crate.
    // Remove default ports.
    if let Some(port) = u.port() {
        let default = if u.scheme() == "https" { 443 } else { 80 };
        if port == default {
            let _ = u.set_port(None);
        }
    }
    let mut s = u.to_string();
    // Strip a single trailing slash on non-root paths ("…/foo/" -> "…/foo").
    if u.path() != "/" && s.ends_with('/') && u.query().is_none() {
        s.pop();
    }
    Some(s)
}

/// The two scheme variants (https first) of a normalized URL, used to dedupe an http/https pair
/// at add time (prompt.md §11 "http vs https").
pub fn scheme_variants(normalized: &str) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(rest) = normalized.strip_prefix("https://") {
        out.push(normalized.to_string());
        out.push(format!("http://{rest}"));
    } else if let Some(rest) = normalized.strip_prefix("http://") {
        out.push(format!("https://{rest}"));
        out.push(normalized.to_string());
    } else {
        out.push(normalized.to_string());
    }
    out
}

/// Hostname of a URL (for per-host politeness), lowercased. `""` if unparseable.
pub fn host_of(raw: &str) -> String {
    Url::parse(raw)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_ascii_lowercase()))
        .unwrap_or_default()
}

/// Resolve `href` (possibly relative) against `base`.
pub fn resolve(base: &str, href: &str) -> Option<String> {
    let base = Url::parse(base).ok()?;
    base.join(href).ok().map(|u| u.to_string())
}

/// True if `host` is a loopback/private/link-local address or a local hostname - the ranges the
/// SSRF guard rejects unless private access is explicitly allowed (prompt.md §11 "Security").
pub fn is_private_host(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host == "localhost" || host.ends_with(".localhost") || host.ends_with(".local") {
        return true;
    }
    // IP literal? (also handles bracketed IPv6 by stripping brackets)
    let ip_str = host.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = ip_str.parse::<IpAddr>() {
        return match ip {
            IpAddr::V4(v4) => {
                v4.is_loopback()
                    || v4.is_private()
                    || v4.is_link_local()
                    || v4.is_unspecified()
                    || v4.is_broadcast()
                    // 100.64.0.0/10 CGNAT (Tailscale range is 100.x but treated as private LAN)
                    || (v4.octets()[0] == 100 && (64..128).contains(&v4.octets()[1]))
            }
            IpAddr::V6(v6) => {
                v6.is_loopback() || v6.is_unspecified() || {
                    let seg = v6.segments()[0];
                    // fc00::/7 unique-local or fe80::/10 link-local
                    (seg & 0xfe00) == 0xfc00 || (seg & 0xffc0) == 0xfe80
                }
            }
        };
    }
    false
}

/// SSRF guard for user-supplied URLs (discovery, full-text). Rejects private ranges unless
/// `allow_private`. Returns the parsed absolute http(s) URL string on success.
pub fn guard_public_url(raw: &str, allow_private: bool) -> Result<String, String> {
    let normalized = normalize_url(raw).ok_or_else(|| "not a valid http(s) URL".to_string())?;
    if !allow_private {
        let host = host_of(&normalized);
        if host.is_empty() {
            return Err("URL has no host".into());
        }
        if is_private_host(&host) {
            return Err("refusing to fetch a private/loopback address".into());
        }
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_trailing_slash_and_default_port() {
        assert_eq!(
            normalize_url("https://Example.com:443/feed/"),
            Some("https://example.com/feed".to_string())
        );
        assert_eq!(
            normalize_url("http://example.com/"),
            Some("http://example.com/".to_string())
        );
    }

    #[test]
    fn scheme_variants_cover_http_and_https() {
        let v = scheme_variants("https://example.com/feed");
        assert!(v.contains(&"https://example.com/feed".to_string()));
        assert!(v.contains(&"http://example.com/feed".to_string()));
    }

    #[test]
    fn rejects_non_http() {
        assert_eq!(normalize_url("ftp://example.com/x"), None);
        assert_eq!(normalize_url("javascript:alert(1)"), None);
    }

    #[test]
    fn private_hosts_detected() {
        assert!(is_private_host("localhost"));
        assert!(is_private_host("127.0.0.1"));
        assert!(is_private_host("192.168.1.10"));
        assert!(is_private_host("10.0.0.5"));
        assert!(is_private_host("::1"));
        assert!(is_private_host("ollama.local"));
        assert!(!is_private_host("example.com"));
        assert!(!is_private_host("8.8.8.8"));
    }

    #[test]
    fn ssrf_guard_blocks_private_by_default() {
        assert!(guard_public_url("http://127.0.0.1/feed", false).is_err());
        assert!(guard_public_url("http://127.0.0.1/feed", true).is_ok());
        assert!(guard_public_url("https://example.com/feed", false).is_ok());
    }
}

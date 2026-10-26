//! Opt-in web access: a keyless search plus readable page fetch.
//!
//! Enklayve is local-first; this module is only ever reached when the user has
//! turned web access on (see the `web_access` setting). Only the user's typed
//! question is sent out — never their documents. HTML scraping of a third-party
//! search page is inherently fragile and subject to that provider's terms, so
//! the parser is isolated and unit-tested, and the provider is easy to swap.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

fn client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (compatible; Enklayve)")
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .context("could not build web client")
}

/// Search the web via DuckDuckGo's keyless HTML endpoint.
pub async fn search(query: &str, max_results: usize) -> Result<Vec<WebResult>> {
    let resp = client()?
        .get("https://html.duckduckgo.com/html/")
        .query(&[("q", query)])
        .send()
        .await
        .context("web search request failed")?;

    if !resp.status().is_success() {
        anyhow::bail!("web search returned status {}", resp.status());
    }

    let html = resp.text().await.context("could not read search response")?;
    Ok(parse_duckduckgo_html(&html, max_results))
}

/// Fetch a page and return its readable text, truncated to `max_chars`.
pub async fn fetch_readable(url: &str, max_chars: usize) -> Result<String> {
    let resp = client()?
        .get(url)
        .send()
        .await
        .with_context(|| format!("could not fetch {}", url))?;
    let html = resp.text().await.context("could not read page body")?;
    let mut text = html2text::from_read(html.as_bytes(), 100);
    if text.len() > max_chars {
        // Back up to a UTF-8 char boundary so truncate() can't panic mid-character.
        let mut end = max_chars;
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    Ok(text)
}

/// Parse DuckDuckGo HTML result rows. Pure and unit-tested so the fragile part
/// of web search can be verified without the network.
pub fn parse_duckduckgo_html(html: &str, max_results: usize) -> Vec<WebResult> {
    let link_re = regex::Regex::new(
        r#"(?s)<a[^>]*class="[^"]*result__a[^"]*"[^>]*href="([^"]+)"[^>]*>(.*?)</a>"#,
    )
    .expect("valid regex");
    let snip_re = regex::Regex::new(
        r#"(?s)<a[^>]*class="[^"]*result__snippet[^"]*"[^>]*>(.*?)</a>"#,
    )
    .expect("valid regex");

    let snippets: Vec<String> = snip_re
        .captures_iter(html)
        .map(|c| strip_html(&c[1]))
        .collect();

    let mut out = Vec::new();
    for (i, cap) in link_re.captures_iter(html).enumerate() {
        if out.len() >= max_results {
            break;
        }
        let url = decode_result_href(&cap[1]);
        let title = strip_html(&cap[2]);
        if title.is_empty() || url.is_empty() {
            continue;
        }
        let snippet = snippets.get(i).cloned().unwrap_or_default();
        out.push(WebResult { title, url, snippet });
    }
    out
}

/// DuckDuckGo wraps result links as `//duckduckgo.com/l/?uddg=<encoded>&...`.
fn decode_result_href(href: &str) -> String {
    if let Some(idx) = href.find("uddg=") {
        let rest = &href[idx + "uddg=".len()..];
        let enc = rest.split('&').next().unwrap_or(rest);
        return percent_decode(enc);
    }
    if let Some(stripped) = href.strip_prefix("//") {
        format!("https://{}", stripped)
    } else {
        href.to_string()
    }
}

/// Minimal percent-decoding (and `+` to space), enough for extracted URLs.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hi = (bytes[i + 1] as char).to_digit(16);
                let lo = (bytes[i + 2] as char).to_digit(16);
                if let (Some(h), Some(l)) = (hi, lo) {
                    out.push((h * 16 + l) as u8);
                    i += 3;
                    continue;
                }
                out.push(b'%');
                i += 1;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Extract candidate URLs / bare domains from text, each normalized with a
/// scheme. Lets "fetch github.com/foo" be fetched directly.
pub fn extract_urls(text: &str) -> Vec<String> {
    let re = regex::Regex::new(r"(?i)\b((?:https?://)?(?:[a-z0-9-]+\.)+[a-z]{2,}(?:/[^\s]*)?)")
        .expect("valid regex");
    let mut out: Vec<String> = Vec::new();
    for cap in re.captures_iter(text) {
        let raw = cap[1].trim_end_matches(|c: char| ".,);:!?'\"".contains(c));
        if raw.is_empty() {
            continue;
        }
        let url = if raw.to_lowercase().starts_with("http") {
            raw.to_string()
        } else {
            format!("https://{}", raw)
        };
        if !out.contains(&url) {
            out.push(url);
        }
    }
    out
}

/// Strip HTML tags and unescape the common entities, collapsing whitespace.
fn strip_html(s: &str) -> String {
    let tag_re = regex::Regex::new(r"(?s)<[^>]+>").expect("valid regex");
    let no_tags = tag_re.replace_all(s, "");
    let unescaped = no_tags
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&#39;", "'")
        .replace("&amp;", "&");
    unescaped.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_decode_basic() {
        assert_eq!(percent_decode("https%3A%2F%2Fa.com%2Fx"), "https://a.com/x");
        assert_eq!(percent_decode("a+b"), "a b");
    }

    #[test]
    fn strip_html_tags_and_entities() {
        assert_eq!(strip_html("<b>Hello</b> &amp; <i>bye</i>"), "Hello & bye");
    }

    #[test]
    fn decode_ddg_wrapped_href() {
        let href = "//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Fpage&rut=abc";
        assert_eq!(decode_result_href(href), "https://example.com/page");
    }

    #[test]
    fn parse_results_from_sample_html() {
        let html = r#"
            <div class="result">
              <a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Fa">First &amp; title</a>
              <a class="result__snippet">A snippet about the first result.</a>
            </div>
            <div class="result">
              <a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.org%2Fb">Second title</a>
              <a class="result__snippet">Second snippet.</a>
            </div>
        "#;
        let results = parse_duckduckgo_html(html, 5);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "First & title");
        assert_eq!(results[0].url, "https://example.com/a");
        assert_eq!(results[0].snippet, "A snippet about the first result.");
        assert_eq!(results[1].url, "https://example.org/b");
    }

    #[test]
    fn extract_urls_finds_bare_and_full() {
        let urls = extract_urls("please fetch github.com/clay-good and https://example.org/x.");
        assert!(urls.contains(&"https://github.com/clay-good".to_string()));
        assert!(urls.contains(&"https://example.org/x".to_string()));
    }

    #[test]
    fn extract_urls_ignores_plain_words() {
        assert!(extract_urls("summarize the documents please").is_empty());
    }

    #[test]
    fn parse_respects_max_results() {
        let row = r#"<a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fa.com">T</a>"#;
        let html = row.repeat(10);
        assert_eq!(parse_duckduckgo_html(&html, 3).len(), 3);
    }
}

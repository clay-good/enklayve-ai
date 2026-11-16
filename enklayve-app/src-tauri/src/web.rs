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
    // A realistic browser User-Agent: DuckDuckGo's HTML endpoint returns far
    // fewer (or no) results for a non-browser agent.
    reqwest::Client::builder()
        .user_agent(
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 \
             (KHTML, like Gecko) Chrome/125.0.0.0 Safari/537.36",
        )
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .context("could not build web client")
}

/// Search the web with no API key: DuckDuckGo first, then Bing if DuckDuckGo
/// blocks us or finds nothing. Both are plain results pages fetched the way a
/// browser would; only the query text leaves the machine.
pub async fn search(query: &str, max_results: usize) -> Result<Vec<WebResult>> {
    let ddg = search_duckduckgo(query, max_results).await;
    if let Ok(results) = &ddg {
        if !results.is_empty() {
            return ddg;
        }
    }
    match search_bing(query, max_results).await {
        Ok(results) if !results.is_empty() => Ok(results),
        Ok(results) => match ddg {
            // Both answered and neither found anything: that is a real "no results".
            Ok(_) => Ok(results),
            Err(e) => Err(e),
        },
        Err(bing_err) => Err(match ddg {
            Err(ddg_err) => anyhow::anyhow!("{}; {}", ddg_err, bing_err),
            Ok(_) => bing_err,
        }),
    }
}

/// DuckDuckGo's keyless HTML endpoint. It sometimes answers automated clients
/// with a bot-check page (HTTP 202, no results); that is an error, not "nothing
/// found", so the caller falls through to the next provider.
async fn search_duckduckgo(query: &str, max_results: usize) -> Result<Vec<WebResult>> {
    let resp = client()?
        .get("https://html.duckduckgo.com/html/")
        .query(&[("q", query)])
        .send()
        .await
        .context("DuckDuckGo request failed")?;
    let status = resp.status();
    if !status.is_success() {
        anyhow::bail!("DuckDuckGo returned status {}", status);
    }
    let html = resp.text().await.context("could not read DuckDuckGo response")?;
    if is_bot_check(status.as_u16(), &html) {
        anyhow::bail!("DuckDuckGo blocked the request (bot check)");
    }
    Ok(parse_duckduckgo_html(&html, max_results))
}

/// Bing's keyless results page, used when DuckDuckGo blocks or comes up empty.
async fn search_bing(query: &str, max_results: usize) -> Result<Vec<WebResult>> {
    let resp = client()?
        .get("https://www.bing.com/search")
        .header("Accept-Language", "en-US,en;q=0.9")
        .query(&[("q", query)])
        .send()
        .await
        .context("Bing request failed")?;
    let status = resp.status();
    if !status.is_success() {
        anyhow::bail!("Bing returned status {}", status);
    }
    let html = resp.text().await.context("could not read Bing response")?;
    let results = parse_bing_html(&html, max_results);
    if results.is_empty() && html.to_lowercase().contains("captcha") {
        anyhow::bail!("Bing blocked the request (captcha)");
    }
    Ok(results)
}

/// Parse Bing result rows (`<li class="b_algo">`). Pure and unit-tested.
pub fn parse_bing_html(html: &str, max_results: usize) -> Vec<WebResult> {
    let link_re = regex::Regex::new(r#"(?s)<h2[^>]*>\s*<a[^>]*href="([^"]+)"[^>]*>(.*?)</a>"#)
        .expect("valid regex");
    let snip_re = regex::Regex::new(r#"(?s)<div class="b_caption[^"]*"[^>]*>.*?<p[^>]*>(.*?)</p>"#)
        .expect("valid regex");
    let mut out = Vec::new();
    for block in html.split(r#"<li class="b_algo""#).skip(1) {
        if out.len() >= max_results {
            break;
        }
        let block = block.split("</li>").next().unwrap_or(block);
        let Some(cap) = link_re.captures(block) else { continue };
        let url = decode_bing_href(&cap[1].replace("&amp;", "&"));
        let title = strip_html(&cap[2]);
        if title.is_empty() || !url.starts_with("http") {
            continue;
        }
        let snippet = snip_re
            .captures(block)
            .map(|c| strip_html(&c[1]))
            .unwrap_or_default();
        out.push(WebResult { title, url, snippet });
    }
    out
}

/// Bing wraps result links as `bing.com/ck/a?...&u=a1<base64url of the URL>`.
fn decode_bing_href(href: &str) -> String {
    use base64::Engine;
    if href.contains("bing.com/ck/a") {
        if let Some(enc) = href.split(['?', '&']).find_map(|kv| kv.strip_prefix("u=a1")) {
            if let Ok(bytes) = base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(enc.trim_end_matches('='))
            {
                if let Ok(url) = String::from_utf8(bytes) {
                    return url;
                }
            }
        }
    }
    href.to_string()
}

/// True when DuckDuckGo served its anti-bot challenge instead of results.
pub fn is_bot_check(status: u16, html: &str) -> bool {
    !html.contains("result__a") && (status == 202 || html.contains("anomaly"))
}

/// Split a message holding several questions into separate search queries, so
/// "who is X?\nwhat happened to Y?" runs one search per question instead of one
/// muddled search for all of them. Returns at most `max` queries; a message with
/// a single question comes back unchanged.
pub fn split_queries(text: &str, max: usize) -> Vec<String> {
    let mut parts: Vec<String> = Vec::new();
    for line in text.lines() {
        for piece in line.split_inclusive('?') {
            let q = piece.trim();
            // Skip filler like "Test" or "one two three" and tiny chit-chat like
            // "who are you?": a search needs a real question (4+ words) to be
            // worth sending.
            if q.split_whitespace().count() >= 4 {
                if !parts.iter().any(|p| p.eq_ignore_ascii_case(q)) {
                    parts.push(q.to_string());
                }
            }
        }
    }
    if parts.len() <= 1 {
        return vec![text.trim().to_string()];
    }
    parts.truncate(max);
    parts
}

/// Fetch a page and return its readable text, truncated to `max_chars`.
pub async fn fetch_readable(url: &str, max_chars: usize) -> Result<String> {
    let resp = client()?
        .get(url)
        .send()
        .await
        .with_context(|| format!("could not fetch {}", url))?;
    let html = resp.text().await.context("could not read page body")?;
    // Prefer the article's paragraphs; menus and sidebars otherwise eat the
    // whole character budget before any real content (Wikipedia, news sites).
    let paragraphs = extract_paragraphs(&html);
    let mut text = if paragraphs.len() >= 300 {
        paragraphs
    } else {
        html2text::from_read(html.as_bytes(), 100)
    };
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

/// The readable body of a page: the text of its `<p>` paragraphs, one per line,
/// skipping tiny fragments and footnote markers like `[12]`. Pure and tested.
pub fn extract_paragraphs(html: &str) -> String {
    let drop_re = regex::Regex::new(r"(?is)<(script|style|noscript)[^>]*>.*?</(script|style|noscript)>")
        .expect("valid regex");
    let p_re = regex::Regex::new(r"(?is)<p[\s>].*?</p>").expect("valid regex");
    let note_re = regex::Regex::new(r"\[(\d+|[a-z]|citation needed)\]").expect("valid regex");
    let html = drop_re.replace_all(html, "");
    let mut out = String::new();
    for m in p_re.find_iter(&html) {
        let text = note_re.replace_all(&strip_html(m.as_str()), "").trim().to_string();
        if text.len() >= 40 {
            out.push_str(&text);
            out.push('\n');
        }
    }
    out
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
    // Quote-aware, so an attribute value containing ">" doesn't leak into the text.
    let tag_re = regex::Regex::new(r#"(?s)<(?:[^>"']|"[^"]*"|'[^']*')+>"#).expect("valid regex");
    let no_tags = tag_re.replace_all(s, "");
    let unescaped = no_tags
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ");
    // Numeric entities like &#0183; (·) or &#32; (space).
    let num_re = regex::Regex::new(r"&#(\d{1,6});").expect("valid regex");
    let unescaped = num_re
        .replace_all(&unescaped, |c: &regex::Captures| {
            c[1].parse::<u32>().ok().and_then(char::from_u32).map(String::from).unwrap_or_default()
        })
        .replace("&amp;", "&");
    unescaped.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bing_results() {
        let html = r#"<ol><li class="b_algo" data-id><div class="b_tpcn"><a class="tilk" href="https://x"><cite>nvidia.com</cite></a></div><h2><a target="_blank" href="https://www.bing.com/ck/a?!&amp;&amp;p=abc&amp;u=a1aHR0cHM6Ly9pbnZlc3Rvci5udmlkaWEuY29tL25ld3M&amp;ntb=1" h="ID=1"><strong>NVIDIA</strong> Announces CEO Transition</a></h2><div class="b_caption"><p class="b_lineclamp2"><span class="news_dt">3 days ago</span>&nbsp;&#0183;&#32;The board named a new CEO.</p></div></li><li class="b_algo"><h2><a href="https://example.com/b">Second</a></h2></li></ol>"#;
        let r = parse_bing_html(html, 5);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].url, "https://investor.nvidia.com/news");
        assert_eq!(r[0].title, "NVIDIA Announces CEO Transition");
        assert!(r[0].snippet.contains("The board named a new CEO."), "{}", r[0].snippet);
        assert_eq!(r[1].url, "https://example.com/b");
    }

    #[test]
    fn extract_paragraphs_skips_menus() {
        let html = r#"<nav><ul><li><a href="/">Main page</a></li></ul></nav><script>var x = "<p>no</p>";</script>
            <p>Donald Trump is the 47th and current president of the United States.[1] He took office on January 20, 2025.</p>
            <p>Tiny.</p><div><p class="x">The president is the head of state and head of government.[a]</p></div>"#;
        let t = extract_paragraphs(html);
        assert_eq!(
            t,
            "Donald Trump is the 47th and current president of the United States. He took office on January 20, 2025.\nThe president is the head of state and head of government.\n"
        );
    }

    #[test]
    fn detects_bot_check_page() {
        assert!(is_bot_check(202, "<div class=\"anomaly-modal\">"));
        assert!(is_bot_check(200, "<html>anomaly</html>"));
        assert!(!is_bot_check(200, r#"<a class="result__a" href="x">T</a>"#));
    }

    #[test]
    fn split_queries_runs_one_search_per_question() {
        // The exact message from the bug report.
        let msg = "Test\n\none two three\n\nwho are you?\n\nwho am I?\n\nwhat is today’s date?\n\nwho is president of china and us?\n\nwhat happened to FAA this week?\n\nwho is the CEO of Nvidia?";
        assert_eq!(
            split_queries(msg, 5),
            vec![
                "what is today’s date?",
                "who is president of china and us?",
                "what happened to FAA this week?",
                "who is the CEO of Nvidia?",
            ]
        );
    }

    #[test]
    fn split_queries_keeps_single_question_whole() {
        assert_eq!(split_queries("latest rust release notes", 4), vec!["latest rust release notes"]);
        assert_eq!(split_queries("who is the CEO of Nvidia?", 4), vec!["who is the CEO of Nvidia?"]);
    }

    #[test]
    fn split_queries_caps_count() {
        assert_eq!(split_queries("is a here now?\nis b here now?\nis c here now?\nis d here now?", 3).len(), 3);
    }

    #[test]
    fn percent_decode_basic() {
        assert_eq!(percent_decode("https%3A%2F%2Fa.com%2Fx"), "https://a.com/x");
        assert_eq!(percent_decode("a+b"), "a b");
    }

    #[test]
    fn strip_html_tags_and_entities() {
        assert_eq!(strip_html("<b>Hello</b> &amp; <i>bye</i>"), "Hello & bye");
        assert_eq!(strip_html(r#"<span data-mw='{"a":"x>y"}'>Kept</span>"#), "Kept");
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

/// Live checks against the real search sites. Not run by default because they
/// need the network; run with `cargo test --lib web::live -- --ignored --nocapture`.
#[cfg(test)]
mod live {
    use super::*;

    #[tokio::test]
    #[ignore]
    async fn live_search_returns_results() {
        for q in ["who is the CEO of Nvidia?", "who is the president of the united states?", "FAA news this week", "who is president of china and us?"] {
            let r = search(q, 5).await.expect("search failed");
            println!("{q} -> {} results", r.len());
            for x in &r {
                println!("  {} | {}", x.title, x.url);
            }
            assert!(!r.is_empty(), "no results for {q}");
            assert!(r.iter().all(|x| x.url.starts_with("http")));
        }
    }
}


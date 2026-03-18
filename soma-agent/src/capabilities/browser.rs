use crate::capabilities::{param, Capability};
use base64::Engine;
use serde_json::{json, Value};
use soma_common::{CapabilityError, ErrorReason, ActionSchema, CapabilityResult};
use std::process::Command;

/// Browser capability — headless web browsing via curl + scraper + chromium.
pub struct BrowserCapability;

const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) SomaOS/0.9 (AI Browser)";

/// Returns the screenshot path under ~/.cache/soma/, creating the directory if needed.
/// Falls back to /tmp only if HOME is not set.
fn screenshot_path() -> String {
    if let Ok(home) = std::env::var("HOME") {
        let dir = format!("{}/.cache/soma", home);
        let _ = std::fs::create_dir_all(&dir);
        // Set directory permissions to 0o700 so only the owner can read it
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
        }
        format!("{}/browser-screenshot.png", dir)
    } else {
        "/tmp/soma-browser.png".to_string()
    }
}

impl Capability for BrowserCapability {
    fn name(&self) -> &str {
        "browser"
    }

    fn description(&self) -> &str {
        "Headless web browser — navigate URLs, extract page content, search the web, take screenshots"
    }

    fn actions(&self) -> Vec<ActionSchema> {
        vec![
            ActionSchema {
                name: "navigate".to_string(),
                description: "Navigate to a URL: fetch title/content and take a headless screenshot"
                    .to_string(),
                params: vec![param("url", "string", true, "URL to navigate to")],
            },
            ActionSchema {
                name: "get_content".to_string(),
                description: "Fetch a webpage and extract title, readable text, and links"
                    .to_string(),
                params: vec![param("url", "string", true, "URL to fetch")],
            },
            ActionSchema {
                name: "search".to_string(),
                description: "Search the web via DuckDuckGo and return top results".to_string(),
                params: vec![param("query", "string", true, "Search query")],
            },
            ActionSchema {
                name: "screenshot".to_string(),
                description: "Take a headless Chromium screenshot of a URL".to_string(),
                params: vec![param("url", "string", true, "URL to screenshot")],
            },
        ]
    }

    fn execute(&self, action: &str, params: &Value) -> CapabilityResult {
        match action {
            "navigate" => self.navigate(params),
            "get_content" => self.get_content(params),
            "search" => self.search(params),
            "screenshot" => self.screenshot(params),
            _ => cap_err(&format!("Unknown browser action: {}", action)),
        }
    }
}

impl BrowserCapability {
    /// Fetch URL content via curl. Returns raw HTML/text on success.
    fn curl_fetch(&self, url: &str) -> Result<String, String> {
        let out = Command::new("curl")
            .args([
                "-L",
                "-s",
                "-A",
                USER_AGENT,
                "--max-time",
                "15",
                "--max-filesize",
                "2097152", // 2 MB cap
                url,
            ])
            .output()
            .map_err(|e| format!("curl not available: {}", e))?;

        if !out.status.success() && out.stdout.is_empty() {
            return Err(String::from_utf8_lossy(&out.stderr).to_string());
        }
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    }

    /// Parse HTML into (title, body_text, links).
    fn parse_page(&self, html: &str) -> (String, String, Vec<String>) {
        use scraper::{Html, Selector};
        let doc = Html::parse_document(html);

        let title = Selector::parse("title")
            .ok()
            .and_then(|s| doc.select(&s).next())
            .map(|e| e.text().collect::<String>().trim().to_string())
            .unwrap_or_default();

        let text_sel = Selector::parse("p, h1, h2, h3, h4, li, article")
            .unwrap_or_else(|_| Selector::parse("body").expect("'body' is always a valid CSS selector"));
        let mut parts: Vec<String> = doc
            .select(&text_sel)
            .map(|e| e.text().collect::<String>().trim().to_string())
            .filter(|s| !s.is_empty() && s.len() > 20)
            .take(40)
            .collect();
        parts.dedup();
        let text = parts.join("\n");

        let link_sel = Selector::parse("a[href]")
            .unwrap_or_else(|_| Selector::parse("body").expect("'body' is always a valid CSS selector"));
        let links: Vec<String> = doc
            .select(&link_sel)
            .filter_map(|e| e.value().attr("href"))
            .filter(|h| h.starts_with("http"))
            .map(|h| h.to_string())
            .take(10)
            .collect();

        (title, text, links)
    }

    /// Navigate to URL: fetch content + headless screenshot.
    fn navigate(&self, params: &Value) -> CapabilityResult {
        let url = match params["url"].as_str() {
            Some(u) => normalize_url(u),
            None => return cap_err("Missing 'url' parameter"),
        };

        let (title, text, links) = match self.curl_fetch(&url) {
            Ok(html) => self.parse_page(&html),
            Err(e) => (String::new(), format!("Fetch error: {}", e), vec![]),
        };

        let path = screenshot_path();
        let screenshot_b64 = self.chromium_screenshot(&url, &path);

        CapabilityResult {
            success: true,
            data: json!({
                "url": url,
                "title": title,
                "text_snippet": &text[..text.len().min(600)],
                "links": links,
                "screenshot_base64": screenshot_b64,
                "screenshot_path": path,
            }),
            error: None,
        }
    }

    /// Fetch page and extract structured content.
    fn get_content(&self, params: &Value) -> CapabilityResult {
        let url = match params["url"].as_str() {
            Some(u) => normalize_url(u),
            None => return cap_err("Missing 'url' parameter"),
        };

        match self.curl_fetch(&url) {
            Ok(html) => {
                let (title, text, links) = self.parse_page(&html);
                CapabilityResult {
                    success: true,
                    data: json!({
                        "url": url,
                        "title": title,
                        "text": &text[..text.len().min(4000)],
                        "links": links,
                        "char_count": text.len(),
                    }),
                    error: None,
                }
            }
            Err(e) => cap_err(&format!("Fetch failed: {}", e)),
        }
    }

    /// Search DuckDuckGo HTML and return structured results.
    fn search(&self, params: &Value) -> CapabilityResult {
        let query = match params["query"].as_str() {
            Some(q) => q,
            None => return cap_err("Missing 'query' parameter"),
        };

        let encoded: String = query
            .chars()
            .flat_map(|c| match c {
                ' ' => "+".chars().collect::<Vec<_>>(),
                c if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' => vec![c],
                c => format!("%{:02X}", c as u32).chars().collect::<Vec<_>>(),
            })
            .collect();

        let search_url = format!("https://html.duckduckgo.com/html/?q={}", encoded);

        match self.curl_fetch(&search_url) {
            Ok(html) => {
                use scraper::{Html, Selector};
                let doc = Html::parse_document(&html);

                let title_sel = Selector::parse(".result__a").ok();
                let snippet_sel = Selector::parse(".result__snippet").ok();
                let url_sel = Selector::parse(".result__url").ok();

                let titles: Vec<String> = title_sel
                    .as_ref()
                    .map(|s| {
                        doc.select(s)
                            .map(|e| e.text().collect::<String>().trim().to_string())
                            .collect()
                    })
                    .unwrap_or_default();
                let snippets: Vec<String> = snippet_sel
                    .as_ref()
                    .map(|s| {
                        doc.select(s)
                            .map(|e| e.text().collect::<String>().trim().to_string())
                            .collect()
                    })
                    .unwrap_or_default();
                let urls: Vec<String> = url_sel
                    .as_ref()
                    .map(|s| {
                        doc.select(s)
                            .map(|e| e.text().collect::<String>().trim().to_string())
                            .collect()
                    })
                    .unwrap_or_default();

                let results: Vec<Value> = titles
                    .iter()
                    .enumerate()
                    .take(8)
                    .map(|(i, t)| {
                        json!({
                            "title": t,
                            "url": urls.get(i).cloned().unwrap_or_default(),
                            "snippet": snippets.get(i).cloned().unwrap_or_default(),
                        })
                    })
                    .collect();

                CapabilityResult {
                    success: true,
                    data: json!({
                        "query": query,
                        "result_count": results.len(),
                        "results": results,
                    }),
                    error: None,
                }
            }
            Err(e) => cap_err(&format!("Search failed: {}", e)),
        }
    }

    /// Take a headless Chromium screenshot.
    fn screenshot(&self, params: &Value) -> CapabilityResult {
        let url = match params["url"].as_str() {
            Some(u) => normalize_url(u),
            None => return cap_err("Missing 'url' parameter"),
        };

        let path = screenshot_path();
        let b64 = self.chromium_screenshot(&url, &path);
        CapabilityResult {
            success: b64.is_some(),
            data: json!({
                "url": url,
                "screenshot_path": path,
                "screenshot_base64": b64,
            }),
            error: if b64.is_none() {
                Some(CapabilityError::new(ErrorReason::NotFound, "Chromium not found or screenshot failed"))
            } else {
                None
            },
        }
    }

    /// Find an available Chromium binary and take a screenshot of `url`.
    /// Writes to `out_path` (should be under ~/.cache/soma/). Returns base64-encoded PNG on success.
    fn chromium_screenshot(&self, url: &str, out_path: &str) -> Option<String> {
        let bins = [
            "chromium-browser",
            "chromium",
            "google-chrome",
            "google-chrome-stable",
        ];
        let bin = bins.iter().find(|&&b| {
            Command::new("which")
                .arg(b)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        })?;

        let _ = Command::new(bin)
            .args([
                "--headless",
                "--no-sandbox",
                "--disable-gpu",
                "--disable-dev-shm-usage",
                &format!("--screenshot={}", out_path),
                "--window-size=1280,720",
                "--virtual-time-budget=5000",
                url,
            ])
            .output()
            .ok()?;

        let bytes = std::fs::read(out_path).ok()?;
        Some(base64::engine::general_purpose::STANDARD.encode(&bytes))
    }
}

fn normalize_url(url: &str) -> String {
    if url.starts_with("http://") || url.starts_with("https://") {
        url.to_string()
    } else {
        format!("https://{}", url)
    }
}

fn cap_err(msg: &str) -> CapabilityResult {
    CapabilityResult {
        success: false,
        data: Value::Null,
        error: Some(CapabilityError::new(ErrorReason::InternalError, msg)),
    }
}

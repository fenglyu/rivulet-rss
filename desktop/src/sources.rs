use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use mail_parser::MessageParser;
use serde::{Deserialize, Serialize};
use url::Url;

pub const MAX_EMAIL_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_BODY_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Publisher {
    Bloomberg,
    Caixin,
    Scmp,
}

impl Publisher {
    pub const ALL: [Self; 3] = [Self::Bloomberg, Self::Caixin, Self::Scmp];

    pub fn name(self) -> &'static str {
        match self {
            Self::Bloomberg => "Bloomberg",
            Self::Caixin => "财新",
            Self::Scmp => "南华早报",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Bloomberg => "bloomberg",
            Self::Caixin => "caixin",
            Self::Scmp => "scmp",
        }
    }

    pub fn home(self) -> &'static str {
        match self {
            Self::Bloomberg => "https://www.bloomberg.com/",
            Self::Caixin => "https://www.caixin.com/",
            Self::Scmp => "https://www.scmp.com/",
        }
    }

    pub fn domain(self) -> &'static str {
        match self {
            Self::Bloomberg => "bloomberg.com",
            Self::Caixin => "caixin.com",
            Self::Scmp => "scmp.com",
        }
    }

    pub fn from_url(value: &str) -> Result<(Self, Url)> {
        let url = Url::parse(value.trim()).context("请输入完整的新闻网址")?;
        if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() {
            bail!("新闻链接必须使用 HTTPS，且不能包含账号密码");
        }
        let host = url.host_str().context("新闻网址缺少域名")?;
        let publisher = Self::ALL
            .into_iter()
            .find(|source| domain_matches(host, source.domain()))
            .context("目前支持 Bloomberg、财新和南华早报的新闻链接")?;
        Ok((publisher, url))
    }

    pub fn is_article(self, url: &Url) -> bool {
        let path = url.path();
        match self {
            Self::Bloomberg => {
                path.starts_with("/news/articles/")
                    || path.starts_with("/opinion/articles/")
                    || path.starts_with("/news/features/")
                    || path.starts_with("/features/")
            }
            Self::Caixin => path.ends_with(".html") && path.contains("/20"),
            Self::Scmp => path.contains("/article/"),
        }
    }

    fn body_selectors(self) -> &'static [&'static str] {
        match self {
            Self::Bloomberg => &[
                ".body-content",
                ".body-copy",
                "[data-component=article-body]",
                "article",
            ],
            Self::Caixin => &[
                "#Main_Content",
                ".article-content",
                ".articleBox",
                "article",
            ],
            Self::Scmp => &[
                "[data-qa=article-body]",
                ".article-body",
                ".article__body",
                "article",
            ],
        }
    }
}

fn domain_matches(host: &str, domain: &str) -> bool {
    host == domain
        || host
            .strip_suffix(domain)
            .is_some_and(|prefix| prefix.ends_with('.'))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Headline {
    pub title: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArticleCapture {
    pub title: String,
    pub url: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub published_at: Option<String>,
    #[serde(default)]
    pub paragraph_count: usize,
    #[serde(default)]
    pub restricted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    Headline,
    Extracted,
    Restricted,
    Newsletter,
}

impl Access {
    pub fn key(self) -> &'static str {
        match self {
            Self::Headline => "headline",
            Self::Extracted => "extracted",
            Self::Restricted => "restricted",
            Self::Newsletter => "newsletter",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Headline => "待读取正文",
            Self::Extracted => "已提取网页正文",
            Self::Restricted => "需要登录或原站阅读",
            Self::Newsletter => "订阅邮件",
        }
    }

    pub fn from_key(key: &str) -> Self {
        match key {
            "extracted" => Self::Extracted,
            "restricted" => Self::Restricted,
            "newsletter" => Self::Newsletter,
            _ => Self::Headline,
        }
    }
}

impl ArticleCapture {
    /// Conservative classification: an article heading or login page is never a readable body.
    pub fn access(&self) -> Access {
        let lower = format!("{}\n{}", self.title, self.text).to_lowercase();
        let gate = [
            "subscribe to continue",
            "subscribe to read",
            "sign in to continue",
            "verify you are human",
            "verify you're human",
            "are you a robot",
            "unusual activity",
            "access denied",
            "请登录后",
            "订阅后可",
            "订阅后阅读",
            "付费阅读",
            "订阅财新通",
            "购买后阅读",
        ]
        .iter()
        .any(|phrase| lower.contains(phrase));
        if self.restricted || gate {
            Access::Restricted
        } else if self.paragraph_count >= 3
            && self.text.chars().count() >= 300
            && self.text.len() <= MAX_BODY_BYTES
        {
            Access::Extracted
        } else {
            Access::Headline
        }
    }
}

/// Read ordinary links from the rendered page. No private endpoints or hidden article data.
pub fn discovery_script() -> &'static str {
    r#"(() => {
        const seen = new Set();
        const links = [];
        for (const a of document.querySelectorAll('a[href]')) {
            if (a.closest('nav, footer') || a.getClientRects().length === 0) continue;
            const title = (a.innerText || a.getAttribute('aria-label') || '').trim();
            if (title.length < 10 || title.length > 600) continue;
            const url = new URL(a.href, location.href);
            url.hash = ''; url.search = '';
            if (seen.has(url.href)) continue;
            seen.add(url.href);
            links.push({title, url: url.href});
            if (links.length >= 300) break;
        }
        const blocked = /are you a robot|verify you are human|verify you're human|access denied|checking your browser/i.test(document.title + ' ' + (document.querySelector('h1')?.innerText || ''));
        return {url: location.href, links, blocked};
    })()"#
}

pub fn parse_headlines(publisher: Publisher, value: serde_json::Value) -> Result<Vec<Headline>> {
    let page_url = value["url"].as_str().context("无法确认新闻列表所在网站")?;
    let (actual, _) = Publisher::from_url(page_url)?;
    if actual != publisher {
        bail!("网站跳转到了其他新闻源，请在原站完成登录");
    }
    if value["blocked"].as_bool().unwrap_or(false) {
        bail!(
            "网站拦截了后台浏览器；普通 Chrome 登录成功并不保证允许 headless 同步。请在原站阅读，或导入订阅邮件"
        );
    }
    let links: Vec<Headline> = serde_json::from_value(value["links"].clone())?;
    let mut seen = std::collections::HashSet::new();
    Ok(links
        .into_iter()
        .filter_map(|mut headline| {
            let (source, mut url) = Publisher::from_url(&headline.url).ok()?;
            if source != publisher
                || !publisher.is_article(&url)
                || headline.title.trim().is_empty()
            {
                return None;
            }
            url.set_fragment(None);
            url.set_query(None);
            headline.url = url.to_string();
            seen.insert(headline.url.clone()).then_some(headline)
        })
        .take(80)
        .collect())
}

/// Only visible paragraphs in an article container are captured. There is no body-wide fallback.
pub fn capture_script(publisher: Publisher) -> String {
    let selectors = serde_json::to_string(publisher.body_selectors()).unwrap();
    format!(
        r#"(() => {{
        const visible = el => el && el.getClientRects().length > 0
            && getComputedStyle(el).visibility !== 'hidden'
            && getComputedStyle(el).display !== 'none';
        const meta = name => document.querySelector(`meta[property="${{name}}"],meta[name="${{name}}"]`)?.content || '';
        let paragraphs = [];
        for (const selector of {selectors}) {{
            const root = document.querySelector(selector);
            if (!visible(root)) continue;
            paragraphs = Array.from(root.querySelectorAll('p')).filter(el => visible(el)
                && !el.closest('aside,nav,footer,[hidden],[aria-hidden="true"]'))
                .map(el => el.innerText.trim()).filter(Boolean);
            if (paragraphs.length >= 3) break;
        }}
        const restricted = Array.from(document.querySelectorAll(
            '[data-component="paywall"],[data-qa="paywall"],.paywall,.paywall-container,.subscription-wall,#paywall'
        )).some(visible) || /are you a robot|verify you are human|verify you're human|access denied|checking your browser/i.test(document.title + ' ' + (document.querySelector('h1')?.innerText || ''));
        return {{
            url: location.href,
            title: document.querySelector('h1')?.innerText.trim() || meta('og:title') || document.title,
            description: meta('description') || meta('og:description'),
            published_at: meta('article:published_time') || null,
            text: paragraphs.join('\n\n').slice(0, 500000),
            paragraph_count: paragraphs.length,
            restricted
        }};
    }})()"#
    )
}

#[derive(Debug)]
pub struct Newsletter {
    pub publisher: Publisher,
    pub id: String,
    pub title: String,
    pub text: String,
    pub published_at: Option<DateTime<Utc>>,
}

pub fn parse_newsletter(bytes: &[u8]) -> Result<Newsletter> {
    if bytes.len() > MAX_EMAIL_BYTES {
        bail!("邮件超过 8 MiB，请单独导出一封新闻邮件");
    }
    let message = MessageParser::default()
        .parse(bytes)
        .context("无法解析 .eml 邮件")?;
    let sender = message
        .from()
        .and_then(|from| from.first())
        .and_then(|from| from.address())
        .context("邮件缺少发件人地址")?;
    let domain = sender.rsplit('@').next().unwrap_or_default().to_lowercase();
    let publisher = Publisher::ALL
        .into_iter()
        .find(|source| {
            domain_matches(&domain, source.domain())
                || (*source == Publisher::Bloomberg && domain_matches(&domain, "bloomberg.net"))
        })
        .context("请选择 Bloomberg、财新或南华早报发来的 .eml 邮件")?;
    // Prefer plain text: no remote images, tracking pixels, scripts, or mail links are fetched.
    let text = if let Some(text) = message.body_text(0) {
        text.into_owned()
    } else if let Some(html) = message.body_html(0) {
        html2text::from_read(html.as_bytes(), 100).context("无法解析邮件正文")?
    } else {
        bail!("这封邮件没有可读取的正文");
    };
    if text.trim().is_empty() || text.len() > MAX_BODY_BYTES {
        bail!("邮件正文为空或超过 2 MiB");
    }
    let id = message.message_id().map(str::to_owned).unwrap_or_else(|| {
        // An identifier from the whole message prevents repeated imports from duplicating it.
        rivulet::domain::Item::generate_id(publisher.home(), &String::from_utf8_lossy(bytes))
    });
    Ok(Newsletter {
        publisher,
        id,
        text,
        title: message.subject().unwrap_or("订阅邮件").to_owned(),
        published_at: message
            .date()
            .and_then(|date| DateTime::from_timestamp(date.to_timestamp(), 0)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rejects_lookalike_domains_credentials_and_non_https() {
        for url in [
            "https://bloomberg.com.evil.test/news/articles/x",
            "https://evilbloomberg.com/",
            "http://www.caixin.com/",
            "https://user:pass@www.scmp.com/article/1/x",
            "file:///etc/passwd",
        ] {
            assert!(Publisher::from_url(url).is_err(), "{url}");
        }
        assert_eq!(
            Publisher::from_url("https://finance.caixin.com/2026-10-04/101.html")
                .unwrap()
                .0,
            Publisher::Caixin
        );
    }

    #[test]
    fn catalogue_filters_and_deduplicates_articles() {
        let data = json!({"url":"https://www.scmp.com/", "links":[
            {"title":"First story", "url":"https://www.scmp.com/news/article/123/story?utm_source=email"},
            {"title":"Same story", "url":"https://www.scmp.com/news/article/123/story#comments"},
            {"title":"Login", "url":"https://www.scmp.com/user/login"},
            {"title":"Other site", "url":"https://www.scmp.com.evil.test/article/1/story"}
        ]});
        let headlines = parse_headlines(Publisher::Scmp, data).unwrap();
        assert_eq!(headlines.len(), 1);
        assert_eq!(
            headlines[0].url,
            "https://www.scmp.com/news/article/123/story"
        );
        assert!(
            parse_headlines(
                Publisher::Scmp,
                json!({"url":"https://evil.test/", "links":[]})
            )
            .is_err()
        );
    }

    #[test]
    fn paywall_and_short_previews_never_count_as_readable_articles() {
        let mut capture = ArticleCapture {
            title: "A real headline".into(),
            url: Publisher::Bloomberg.home().into(),
            description: String::new(),
            text: "A useful paragraph. ".repeat(40),
            paragraph_count: 4,
            published_at: None,
            restricted: false,
        };
        assert_eq!(capture.access(), Access::Extracted);
        capture.restricted = true;
        assert_eq!(capture.access(), Access::Restricted);
        capture.restricted = false;
        capture.text.push_str("Subscribe to continue");
        assert_eq!(capture.access(), Access::Restricted);
        capture.text = "A short teaser".into();
        assert_eq!(capture.access(), Access::Headline);
        capture.title = "Bloomberg - Are you a robot?".into();
        assert_eq!(capture.access(), Access::Restricted);
    }

    #[test]
    fn robot_pages_are_rejected_before_headline_import() {
        let data = json!({"url":Publisher::Bloomberg.home(), "blocked":true, "links":[
            {"title":"An otherwise valid link", "url":"https://www.bloomberg.com/news/articles/2026-10-04/test-story"}
        ]});
        let error = parse_headlines(Publisher::Bloomberg, data).unwrap_err();
        assert!(error.to_string().contains("后台浏览器"));
    }

    #[test]
    fn decodes_newsletters_and_preserves_stable_identity() {
        let email = b"From: Bloomberg <news@news.bloomberg.com>\r\nSubject: =?UTF-8?B?5biC5Zy66YCa5oql?=\r\nMessage-ID: <digest-42@bloomberg.com>\r\nDate: Sun, 04 Oct 2026 08:00:00 +0800\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nYour subscriber newsletter.\r\n";
        let newsletter = parse_newsletter(email).unwrap();
        assert_eq!(newsletter.publisher, Publisher::Bloomberg);
        assert_eq!(newsletter.title, "市场通报");
        assert_eq!(newsletter.id, "digest-42@bloomberg.com");
        assert!(newsletter.published_at.is_some());
        assert!(parse_newsletter(b"From: x@evil.test\r\n\r\nUnknown newsletter").is_err());
    }
}

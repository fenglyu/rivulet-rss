use std::{collections::HashMap, path::PathBuf, sync::Arc};

use anyhow::{Context, Result, bail};
use async_channel::{Receiver, Sender};
use chrono::Utc;
use rivulet::{
    app::AppContext,
    domain::{AuthProfile, Feed, Item},
    scraper::{ChromeScraper, ScraperConfig},
    store::{ItemListFilter, Store},
};
use rusqlite::{Connection, OptionalExtension, params};

use crate::sources::{
    Access, ArticleCapture, MAX_EMAIL_BYTES, Publisher, capture_script, discovery_script,
    parse_headlines, parse_newsletter,
};

#[derive(Debug, Clone)]
pub struct Article {
    pub item: Item,
    pub publisher: Publisher,
    pub access: Access,
    pub is_read: bool,
    pub is_saved: bool,
}

#[derive(Debug, Clone)]
pub struct SourceInfo {
    pub publisher: Publisher,
    pub status: String,
    pub count: usize,
    pub auto_sync: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub articles: Vec<Article>,
    pub sources: Vec<SourceInfo>,
}

#[derive(Debug, Clone)]
pub struct Filter {
    pub publisher: Option<Publisher>,
    pub view: ItemListFilter,
    pub query: String,
}

impl Default for Filter {
    fn default() -> Self {
        Self {
            publisher: None,
            view: ItemListFilter::All,
            query: String::new(),
        }
    }
}

pub enum Command {
    Load(Filter),
    Login(Publisher),
    Sync(Publisher),
    Capture(String),
    OpenArticle(String),
    ImportEmails(Vec<PathBuf>),
    SetRead(String, bool),
    SetSaved(String, bool),
    SetAutoSync(Publisher, bool),
    Shutdown(Sender<()>),
}

pub enum Event {
    Busy(String),
    Updated(Snapshot, String),
    Failed(String),
}

pub struct Worker {
    pub commands: Sender<Command>,
    pub events: Receiver<Event>,
}

pub fn default_data_dir() -> Result<PathBuf> {
    Ok(dirs::data_local_dir()
        .context("无法定位本地数据目录")?
        .join("rivulet-desktop"))
}

impl Worker {
    pub fn start(data_dir: PathBuf) -> Self {
        let (commands, commands_rx) = async_channel::bounded(128);
        let (events_tx, events) = async_channel::unbounded();
        std::thread::spawn(move || {
            let runtime = match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    let _ = events_tx.send_blocking(Event::Failed(error.to_string()));
                    return;
                }
            };
            runtime.block_on(async move {
                let mut engine = match Engine::new(data_dir) {
                    Ok(engine) => engine,
                    Err(error) => {
                        let _ = events_tx
                            .send(Event::Failed(format!("初始化失败：{error:#}")))
                            .await;
                        return;
                    }
                };
                let mut refresh = tokio::time::interval(std::time::Duration::from_secs(15 * 60));
                refresh.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                refresh.tick().await;
                loop {
                    let command = tokio::select! {
                        command = commands_rx.recv() => match command { Ok(command) => command, Err(_) => break },
                        _ = refresh.tick() => {
                            for publisher in Publisher::ALL {
                                if !engine.auto_sync_enabled(publisher).unwrap_or(false) { continue; }
                                if events_tx.send(Event::Busy(format!("正在同步 {}…", publisher.name()))).await.is_err() { break; }
                                let message = match engine.execute(Command::Sync(publisher)).await {
                                    Ok(message) => message,
                                    Err(error) => format!("{} 自动同步暂停：{error:#}", publisher.name()),
                                };
                                if let Ok(snapshot) = engine.snapshot() {
                                    let _ = events_tx.send(Event::Updated(snapshot, message)).await;
                                }
                            }
                            continue;
                        }
                    };
                    if let Command::Shutdown(done) = command {
                        for (_, (_, browser)) in engine.sessions.drain() { let _ = browser.close().await; }
                        let _ = done.send(()).await;
                        break;
                    }
                    if events_tx
                        .send(Event::Busy("正在处理…".into()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                    let result = engine.execute(command).await;
                    let event = match result
                        .and_then(|message| engine.snapshot().map(|snapshot| (snapshot, message)))
                    {
                        Ok((snapshot, message)) => Event::Updated(snapshot, message),
                        Err(error) => match engine.snapshot() {
                            Ok(snapshot) => Event::Updated(snapshot, format!("{error:#}")),
                            Err(_) => Event::Failed(format!("{error:#}")),
                        },
                    };
                    if events_tx.send(event).await.is_err() {
                        break;
                    }
                }
                for (_, (_, browser)) in engine.sessions.drain() { let _ = browser.close().await; }
            });
        });
        Self { commands, events }
    }
}

struct Engine {
    ctx: Arc<AppContext>,
    meta: Connection,
    data_dir: PathBuf,
    sessions: HashMap<Publisher, (bool, ChromeScraper)>,
    filter: Filter,
}

impl Engine {
    fn new(data_dir: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&data_dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&data_dir, std::fs::Permissions::from_mode(0o700))?;
        }
        let db_path = data_dir.join("rivulet-desktop.db");
        let ctx = Arc::new(AppContext::new(Some(db_path.clone()))?);
        let meta = Connection::open(db_path)?;
        meta.execute_batch(
            "PRAGMA foreign_keys = ON;
            CREATE TABLE IF NOT EXISTS desktop_article_meta (
                item_id TEXT PRIMARY KEY REFERENCES items(id) ON DELETE CASCADE,
                publisher TEXT NOT NULL, access TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS desktop_sources (
                publisher TEXT PRIMARY KEY, status TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS desktop_auto_sync (
                publisher TEXT PRIMARY KEY, enabled INTEGER NOT NULL DEFAULT 0
            );",
        )?;
        Ok(Self {
            ctx,
            meta,
            data_dir,
            sessions: HashMap::new(),
            filter: Filter::default(),
        })
    }

    async fn execute(&mut self, command: Command) -> Result<String> {
        match command {
            Command::Load(filter) => {
                self.filter = filter;
                Ok("本地资料已载入".into())
            }
            Command::Login(publisher) => {
                let browser = self.session(publisher, false).await?;
                browser.open_interactive_page(publisher.home()).await?;
                self.set_source_status(publisher, "登录窗口已打开 · 会话尚未验证")?;
                Ok(format!(
                    "请在 Chrome 中登录 {}，完成后点击“同步标题”",
                    publisher.name()
                ))
            }
            Command::Sync(publisher) => {
                let result = self.sync(publisher).await;
                if result.is_err() {
                    self.set_source_status(publisher, "同步失败 · 请登录后重试")?;
                    self.set_auto_sync(publisher, false)?;
                    if let Some((_, browser)) = self.sessions.remove(&publisher) {
                        let _ = browser.close().await;
                    }
                }
                result
            }
            Command::Capture(url) => self.capture(&url).await,
            Command::OpenArticle(url) => {
                let (publisher, url) = Publisher::from_url(&url)?;
                self.session(publisher, false)
                    .await?
                    .open_interactive_page(url.as_str())
                    .await?;
                Ok("文章已在该新闻源的登录会话中打开".into())
            }
            Command::ImportEmails(paths) => {
                let mut imported = 0;
                let mut duplicates = 0;
                let mut errors = Vec::new();
                for path in paths {
                    match self.import_email(&path) {
                        Ok(true) => imported += 1,
                        Ok(false) => duplicates += 1,
                        Err(error) => errors.push(format!(
                            "{}：{error}",
                            path.file_name().unwrap_or_default().to_string_lossy()
                        )),
                    }
                }
                Ok(format!(
                    "导入 {imported} 封，已有 {duplicates} 封{}",
                    if errors.is_empty() {
                        String::new()
                    } else {
                        format!("；{}", errors.join("；"))
                    }
                ))
            }
            Command::SetRead(id, value) => {
                self.ctx.store.set_read(&id, value)?;
                Ok("阅读状态已保存".into())
            }
            Command::SetSaved(id, value) => {
                self.ctx.store.set_saved(&id, value)?;
                Ok("收藏状态已保存".into())
            }
            Command::SetAutoSync(publisher, enabled) => {
                self.set_auto_sync(publisher, enabled)?;
                Ok(if enabled {
                    "已启用每 15 分钟同步标题"
                } else {
                    "已暂停自动同步"
                }
                .into())
            }
            Command::Shutdown(_) => unreachable!("shutdown is handled by the worker"),
        }
    }

    fn feed_id(&self, publisher: Publisher, newsletter: bool) -> Result<i64> {
        let url = if newsletter {
            format!("{}#newsletters", publisher.home())
        } else {
            publisher.home().into()
        };
        if let Some(feed) = self.ctx.store.get_feed_by_url(&url)? {
            return Ok(feed.id);
        }
        let mut feed = Feed::new(url);
        feed.title = Some(format!(
            "{}{}",
            publisher.name(),
            if newsletter { " · 邮件" } else { "" }
        ));
        Ok(self.ctx.store.add_feed(&feed)?)
    }

    fn set_source_status(&self, publisher: Publisher, status: &str) -> Result<()> {
        self.meta.execute(
            "INSERT INTO desktop_sources(publisher,status) VALUES(?1,?2)
            ON CONFLICT(publisher) DO UPDATE SET status=excluded.status",
            params![publisher.key(), status],
        )?;
        Ok(())
    }

    fn set_auto_sync(&self, publisher: Publisher, enabled: bool) -> Result<()> {
        self.meta.execute(
            "INSERT INTO desktop_auto_sync(publisher,enabled) VALUES(?1,?2)
            ON CONFLICT(publisher) DO UPDATE SET enabled=excluded.enabled",
            params![publisher.key(), enabled],
        )?;
        Ok(())
    }

    fn auto_sync_enabled(&self, publisher: Publisher) -> Result<bool> {
        Ok(self
            .meta
            .query_row(
                "SELECT enabled FROM desktop_auto_sync WHERE publisher=?1",
                params![publisher.key()],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(false))
    }

    fn set_meta(&self, id: &str, publisher: Publisher, access: Access) -> Result<()> {
        self.meta.execute(
            "INSERT INTO desktop_article_meta(item_id,publisher,access) VALUES(?1,?2,?3)
            ON CONFLICT(item_id) DO UPDATE SET access=excluded.access",
            params![id, publisher.key(), access.key()],
        )?;
        Ok(())
    }

    async fn session(&mut self, publisher: Publisher, headless: bool) -> Result<&ChromeScraper> {
        if self
            .sessions
            .get(&publisher)
            .is_some_and(|(mode, _)| *mode != headless)
            && let Some((_, browser)) = self.sessions.remove(&publisher)
        {
            browser
                .close()
                .await
                .context("无法切换浏览器模式，请关闭该新闻源的登录窗口后重试")?;
        }
        if !self.sessions.contains_key(&publisher) {
            let profile_dir = self.data_dir.join("profiles").join(publisher.key());
            std::fs::create_dir_all(&profile_dir)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&profile_dir, std::fs::Permissions::from_mode(0o700))?;
            }
            let profile = AuthProfile::new(
                publisher.key().into(),
                publisher.home().into(),
                profile_dir.to_string_lossy().into(),
            );
            self.ctx.store.add_auth_profile(&profile)?;
            let browser = ChromeScraper::new(ScraperConfig {
                headless,
                user_data_dir: Some(profile_dir),
                restore_session: true,
                user_agent: None,
                block_images: false,
                block_stylesheets: false,
                max_concurrency: 1,
                timeout_secs: 45,
                wait_after_load_ms: 2000,
                ..ScraperConfig::default()
            })
            .await
            .context("无法打开订阅会话，请确认已安装 Chrome 或 Chromium")?;
            self.sessions.insert(publisher, (headless, browser));
        }
        self.sessions
            .get(&publisher)
            .map(|(_, browser)| browser)
            .context("无法建立本地会话")
    }

    async fn sync(&mut self, publisher: Publisher) -> Result<String> {
        let browser = self.session(publisher, true).await?;
        let value = browser
            .evaluate_page(publisher.home(), discovery_script())
            .await?;
        let headlines = parse_headlines(publisher, value)?;
        if headlines.is_empty() {
            bail!(
                "{} 页面没有可识别的新闻链接。请在原站处理登录或验证，也可直接粘贴文章链接。",
                publisher.name()
            );
        }
        let feed_id = self.feed_id(publisher, false)?;
        let mut added = 0;
        for headline in headlines {
            let mut item = Item::new(feed_id, publisher.home(), &headline.url);
            if self.ctx.store.item_exists(&item.id)? {
                continue;
            }
            item.title = Some(headline.title);
            item.link = Some(headline.url);
            self.ctx.store.add_item(&item)?;
            self.set_meta(&item.id, publisher, Access::Headline)?;
            added += 1;
        }
        self.set_source_status(
            publisher,
            &format!("已同步标题 · {}", Utc::now().format("%m-%d %H:%M UTC")),
        )?;
        self.set_auto_sync(publisher, true)?;
        Ok(format!(
            "{} 新增 {added} 条标题，文章正文可逐篇读取",
            publisher.name()
        ))
    }

    async fn capture(&mut self, value: &str) -> Result<String> {
        let (publisher, mut url) = Publisher::from_url(value)?;
        if !publisher.is_article(&url) {
            bail!("请粘贴具体文章链接，网站首页请使用“同步标题”");
        }
        url.set_fragment(None);
        url.set_query(None);
        let url = url.to_string();
        let feed_id = self.feed_id(publisher, false)?;
        let mut item = Item::new(feed_id, publisher.home(), &url);
        item.title = Some(url.clone());
        item.link = Some(url.clone());
        if !self.ctx.store.item_exists(&item.id)? {
            self.ctx.store.add_item(&item)?;
            self.set_meta(&item.id, publisher, Access::Headline)?;
        }
        let browser = self.session(publisher, true).await?;
        let capture: ArticleCapture = serde_json::from_value(
            browser
                .evaluate_page(&url, &capture_script(publisher))
                .await?,
        )?;
        let (actual_source, mut actual_url) = Publisher::from_url(&capture.url)?;
        actual_url.set_query(None);
        actual_url.set_fragment(None);
        if actual_source != publisher || actual_url != url::Url::parse(&url)? {
            bail!("文章跳转到了登录页或其他页面，已保留链接，请完成原站登录后重试");
        }
        let access = capture.access();
        // Keep existing readable content when a later session expires.
        if let Some(existing) = self.ctx.store.get_item(&item.id)? {
            item = existing;
        }
        if access == Access::Restricted {
            if item.content.is_none() {
                self.set_meta(&item.id, publisher, access)?;
            }
            self.set_source_status(publisher, "文章要求登录、订阅或验证")?;
            return Ok(
                "原站要求登录、订阅或验证。请在 Chrome 处理后重新读取；已保留文章链接和已有内容。"
                    .into(),
            );
        }
        if !capture.title.trim().is_empty() {
            item.title = Some(capture.title);
        }
        if !capture.description.is_empty() {
            item.summary = Some(capture.description);
        }
        item.published_at = capture
            .published_at
            .and_then(|date| chrono::DateTime::parse_from_rfc3339(&date).ok())
            .map(|date| date.with_timezone(&Utc));
        // add_item is insert-only; preserve metadata with a dedicated update for this adapter.
        self.meta.execute("UPDATE items SET title=?2,summary=?3,published_at=COALESCE(?4,published_at) WHERE id=?1",
            params![item.id, item.title, item.summary, item.published_at.map(|date| date.to_rfc3339())])?;
        if access == Access::Extracted {
            self.ctx
                .store
                .update_item_content(&item.id, &capture.text)?;
            self.set_meta(&item.id, publisher, access)?;
            self.set_source_status(publisher, "已提取文章正文 · 当前会话可用")?;
        } else if item.content.is_none() {
            // Rebuild the FTS index without inventing a readable body.
            self.ctx.store.update_item_content(&item.id, "")?;
            self.meta.execute(
                "UPDATE items SET content=NULL WHERE id=?1",
                params![item.id],
            )?;
            self.set_meta(&item.id, publisher, access)?;
        }
        Ok(match access {
            Access::Extracted => "已保存当前页面可见正文，可离线阅读".into(),
            Access::Restricted => {
                "原站要求登录、订阅或验证。请在 Chrome 处理后重新读取；文章链接已保存。".into()
            }
            _ => "已保存标题和摘要，未确认可读正文；可在原站打开".into(),
        })
    }

    fn import_email(&self, path: &std::path::Path) -> Result<bool> {
        if !path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("eml"))
        {
            bail!("请选择 .eml 文件");
        }
        use std::io::Read;
        let mut bytes = Vec::new();
        std::fs::File::open(path)?
            .take((MAX_EMAIL_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        let newsletter = parse_newsletter(&bytes)?;
        let feed_id = self.feed_id(newsletter.publisher, true)?;
        let mut item = Item::new(
            feed_id,
            &format!("{}#newsletters", newsletter.publisher.home()),
            &newsletter.id,
        );
        if self.ctx.store.item_exists(&item.id)? {
            return Ok(false);
        }
        item.title = Some(newsletter.title);
        item.content = Some(newsletter.text);
        item.published_at = newsletter.published_at;
        self.ctx.store.add_item(&item)?;
        self.set_meta(&item.id, newsletter.publisher, Access::Newsletter)?;
        Ok(true)
    }

    fn snapshot(&self) -> Result<Snapshot> {
        // Literal matching supports Chinese substrings and names such as BRK.B
        // without passing user input to the FTS query language.
        let terms: Vec<String> = self
            .filter
            .query
            .split_whitespace()
            .map(str::to_lowercase)
            .collect();
        let mut articles = Vec::new();
        for item in self.ctx.store.get_items_by_filter(self.filter.view)? {
            let metadata: Option<(String, String)> = self
                .meta
                .query_row(
                    "SELECT publisher,access FROM desktop_article_meta WHERE item_id=?1",
                    params![item.id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;
            let Some((publisher, access)) = metadata else {
                continue;
            };
            let Some(publisher) = Publisher::ALL
                .into_iter()
                .find(|source| source.key() == publisher)
            else {
                continue;
            };
            if self
                .filter
                .publisher
                .is_some_and(|source| source != publisher)
            {
                continue;
            }
            if !terms.is_empty() {
                let searchable = format!(
                    "{}\n{}\n{}\n{}",
                    item.title.as_deref().unwrap_or_default(),
                    item.summary.as_deref().unwrap_or_default(),
                    item.content.as_deref().unwrap_or_default(),
                    item.author.as_deref().unwrap_or_default()
                )
                .to_lowercase();
                if !terms.iter().all(|term| searchable.contains(term)) {
                    continue;
                }
            }
            let state = self.ctx.store.get_item_state(&item.id)?;
            articles.push(Article {
                item,
                publisher,
                access: Access::from_key(&access),
                is_read: state.as_ref().is_some_and(|state| state.is_read),
                is_saved: state.as_ref().is_some_and(|state| state.is_saved),
            });
            if articles.len() >= 500 {
                break;
            }
        }
        let mut sources = Vec::new();
        for publisher in Publisher::ALL {
            let status = self
                .meta
                .query_row(
                    "SELECT status FROM desktop_sources WHERE publisher=?1",
                    params![publisher.key()],
                    |row| row.get(0),
                )
                .optional()?
                .unwrap_or_else(|| "尚未连接".into());
            let count = self.meta.query_row(
                "SELECT COUNT(*) FROM desktop_article_meta WHERE publisher=?1",
                params![publisher.key()],
                |row| row.get::<_, i64>(0),
            )?;
            sources.push(SourceInfo {
                publisher,
                status,
                count: usize::try_from(count)?,
                auto_sync: self.auto_sync_enabled(publisher)?,
            });
        }
        Ok(Snapshot { articles, sources })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newsletter_import_is_persistent_deduplicated_and_searchable() {
        let dir = tempfile::tempdir().unwrap();
        let mail = dir.path().join("daily.eml");
        std::fs::write(&mail, "From: digest@caixin.com\r\nSubject: China research\r\nMessage-ID: <same@caixin.com>\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nSemiconductor policy changed today. 中国半导体政策。BRK.B earnings.").unwrap();
        {
            let engine = Engine::new(dir.path().join("data")).unwrap();
            assert!(engine.import_email(&mail).unwrap());
            assert!(!engine.import_email(&mail).unwrap());
            let snapshot = engine.snapshot().unwrap();
            assert_eq!(snapshot.articles.len(), 1);
            assert_eq!(snapshot.articles[0].access, Access::Newsletter);
            engine
                .ctx
                .store
                .set_saved(&snapshot.articles[0].item.id, true)
                .unwrap();
        }
        let mut engine = Engine::new(dir.path().join("data")).unwrap();
        engine.filter.query = "semiconductor".into();
        let snapshot = engine.snapshot().unwrap();
        assert_eq!(snapshot.articles.len(), 1);
        assert!(snapshot.articles[0].is_saved);
        for query in ["半导体", "BRK.B", "中国 earnings", "SEMICONDUCTOR"] {
            engine.filter.query = query.into();
            assert_eq!(engine.snapshot().unwrap().articles.len(), 1, "{query}");
        }
        engine.filter.query = "missing earnings".into();
        assert!(engine.snapshot().unwrap().articles.is_empty());
        engine.filter.query.clear();
        engine.filter.publisher = Some(Publisher::Scmp);
        assert!(engine.snapshot().unwrap().articles.is_empty());
    }
}

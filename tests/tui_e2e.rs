use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    backend::{Backend, ClearType, TestBackend, WindowSize},
    buffer::Buffer,
    layout::{Position, Size},
    Terminal,
};
use rivulet::app::context::AppContext;
use rivulet::config::Config;
use rivulet::domain::{Feed, Item};
use rivulet::fetcher::testing::MockFetcher;
use rivulet::fetcher::FetchResult;
use rivulet::store::Store;
use rivulet::tui::event::{AppEvent, EventHandler};
use rivulet::tui::run_app;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

/// A backend that wraps `TestBackend` and republishes each rendered buffer to
/// an mpsc channel so tests can synchronize via `step_until`.
struct ObservableBackend {
    inner: TestBackend,
    tx: mpsc::UnboundedSender<Buffer>,
}

impl ObservableBackend {
    fn new(width: u16, height: u16) -> (Self, mpsc::UnboundedReceiver<Buffer>) {
        let inner = TestBackend::new(width, height);
        let (tx, rx) = mpsc::unbounded_channel();
        let _ = tx.send(inner.buffer().clone());
        (Self { inner, tx }, rx)
    }
}

impl Backend for ObservableBackend {
    type Error = std::convert::Infallible;

    fn draw<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = (u16, u16, &'a ratatui::buffer::Cell)>,
    {
        self.inner.draw(content).unwrap();
        let _ = self.tx.send(self.inner.buffer().clone());
        Ok(())
    }

    fn hide_cursor(&mut self) -> Result<(), Self::Error> {
        self.inner.hide_cursor().unwrap();
        Ok(())
    }
    fn show_cursor(&mut self) -> Result<(), Self::Error> {
        self.inner.show_cursor().unwrap();
        Ok(())
    }
    fn get_cursor_position(&mut self) -> Result<Position, Self::Error> {
        Ok(self.inner.get_cursor_position().unwrap())
    }
    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> Result<(), Self::Error> {
        self.inner.set_cursor_position(position).unwrap();
        Ok(())
    }
    fn clear(&mut self) -> Result<(), Self::Error> {
        self.inner.clear().unwrap();
        Ok(())
    }
    fn clear_region(&mut self, clear_type: ClearType) -> Result<(), Self::Error> {
        self.inner.clear_region(clear_type).unwrap();
        Ok(())
    }
    fn size(&self) -> Result<Size, Self::Error> {
        Ok(self.inner.size().unwrap())
    }
    fn window_size(&mut self) -> Result<WindowSize, Self::Error> {
        Ok(self.inner.window_size().unwrap())
    }
    fn flush(&mut self) -> Result<(), Self::Error> {
        self.inner.flush().unwrap();
        Ok(())
    }
}

struct Harness {
    tx: mpsc::UnboundedSender<AppEvent>,
    buffer_rx: mpsc::UnboundedReceiver<Buffer>,
    join_handle: tokio::task::JoinHandle<()>,
    last_buffer: Option<Buffer>,
}

impl Harness {
    async fn setup(ctx: Arc<AppContext>) -> Self {
        let config = Arc::new(Config::default());
        let (backend, buffer_rx) = ObservableBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        let event_handler = EventHandler::new(Duration::from_millis(10));
        let tx = event_handler.get_tx();

        let join_handle = tokio::spawn(async move {
            run_app(&mut terminal, event_handler, ctx, config)
                .await
                .unwrap();
        });

        Self {
            tx,
            buffer_rx,
            join_handle,
            last_buffer: None,
        }
    }

    fn send_key(&self, code: KeyCode) {
        self.send_key_with(code, KeyModifiers::NONE);
    }

    fn send_key_with(&self, code: KeyCode, mods: KeyModifiers) {
        let key = KeyEvent::new(code, mods);
        self.tx.send(AppEvent::Key(key)).unwrap();
    }

    /// Drain rendered buffers until `predicate` is satisfied or `timeout` elapses.
    /// Caches the latest seen buffer so callers can re-check after sending more keys.
    async fn step_until<F>(&mut self, mut predicate: F, timeout: Duration) -> bool
    where
        F: FnMut(&Buffer) -> bool,
    {
        let start = std::time::Instant::now();

        if let Some(ref buffer) = self.last_buffer {
            if predicate(buffer) {
                return true;
            }
        }

        while start.elapsed() < timeout {
            let remaining = timeout
                .checked_sub(start.elapsed())
                .unwrap_or_else(|| Duration::from_millis(1));
            match tokio::time::timeout(remaining, self.buffer_rx.recv()).await {
                Ok(Some(buffer)) => {
                    let matched = predicate(&buffer);
                    self.last_buffer = Some(buffer);
                    if matched {
                        return true;
                    }
                }
                _ => break,
            }
        }
        false
    }

    async fn quit(mut self) {
        self.send_key(KeyCode::Char('q'));
        // Drain any remaining buffers so run_app's render channel doesn't back up.
        let _ = tokio::time::timeout(Duration::from_secs(2), async {
            while self.buffer_rx.recv().await.is_some() {}
        })
        .await;
        let _ = tokio::time::timeout(Duration::from_secs(2), self.join_handle).await;
    }
}

fn add_feed_with_items(ctx: &AppContext, title: &str, item_count: usize) -> i64 {
    let mut feed = Feed::new(format!("https://example.com/{title}.xml"));
    feed.title = Some(title.to_string());
    let feed_id = ctx.store.add_feed(&feed).unwrap();

    for index in 0..item_count {
        let mut item = Item::new(feed_id, &feed.url, &format!("{title}-entry-{index}"));
        item.title = Some(format!("{title} item {index}"));
        ctx.store.add_item(&item).unwrap();
    }

    feed_id
}

fn buffer_to_string(buffer: &Buffer) -> String {
    let mut out = String::new();
    for y in 0..buffer.area().height {
        for x in 0..buffer.area().width {
            out.push_str(buffer[(x, y)].symbol());
        }
        out.push('\n');
    }
    out
}

#[tokio::test]
async fn test_layout_collapse_rail() {
    let ctx = Arc::new(AppContext::in_memory().unwrap());
    add_feed_with_items(&ctx, "alpha", 1);
    let mut harness = Harness::setup(ctx).await;

    harness.send_key(KeyCode::Char(']')); // Reader tab → expanded rail by default.

    // Expanded: title " Feeds " is wide enough to render.
    assert!(
        harness
            .step_until(
                |b| buffer_to_string(b).contains(" Feeds "),
                Duration::from_secs(2),
            )
            .await,
        "expanded rail should render the ' Feeds ' title"
    );

    harness.send_key(KeyCode::Char('\\')); // Collapse.

    // Collapsed rail is 3 cells wide: │F│. The body starts at row 1 (row 0 is
    // the tab strip), so the top border sits at row 1 and the paragraph 'F'
    // sits at row 2 column 1. Assert title gone AND the collapsed glyph in place.
    assert!(
        harness
            .step_until(
                |b| {
                    let title_gone = !buffer_to_string(b).contains(" Feeds ");
                    let collapsed_glyph = b[(1, 2)].symbol() == "F";
                    title_gone && collapsed_glyph
                },
                Duration::from_secs(2),
            )
            .await,
        "collapsed rail should show single 'F' at (1, 2)"
    );

    harness.quit().await;
}

#[tokio::test]
async fn test_layout_maximize_preview() {
    let ctx = Arc::new(AppContext::in_memory().unwrap());
    add_feed_with_items(&ctx, "alpha", 1);
    let mut harness = Harness::setup(ctx).await;

    harness.send_key(KeyCode::Char(']')); // Reader.
    harness.send_key(KeyCode::Enter); // Feeds → Items.
    harness.send_key(KeyCode::Enter); // Items → Preview.

    assert!(
        harness
            .step_until(
                |b| {
                    let s = buffer_to_string(b);
                    s.contains("Preview") || s.contains("alpha item 0")
                },
                Duration::from_secs(2),
            )
            .await,
        "preview pane should be visible"
    );

    harness.send_key(KeyCode::Char('m')); // Maximize.

    assert!(
        harness
            .step_until(
                |b| {
                    let s = buffer_to_string(b);
                    !s.contains(" Feeds ") && !s.contains(" Items: ")
                },
                Duration::from_secs(2),
            )
            .await,
        "maximized preview should hide other panes"
    );

    harness.quit().await;
}

#[tokio::test]
async fn test_visual_markers() {
    let ctx = Arc::new(AppContext::in_memory().unwrap());
    add_feed_with_items(&ctx, "alpha", 1);
    let mut harness = Harness::setup(ctx).await;

    harness.send_key(KeyCode::Char(']'));
    assert!(
        harness
            .step_until(
                |b| buffer_to_string(b).contains("alpha item 0"),
                Duration::from_secs(2),
            )
            .await,
        "items should load"
    );

    harness.send_key(KeyCode::Char('l')); // Focus Items.
    harness.send_key(KeyCode::Char('s')); // Toggle star.

    // Layout renders markers as 3-char strings (src/tui/layout.rs:319-329).
    // Asserting on the bare glyph would match unrelated cells in titles/borders.
    assert!(
        harness
            .step_until(
                |b| buffer_to_string(b).contains("*  "),
                Duration::from_secs(2),
            )
            .await,
        "starred item should render '*  ' marker"
    );

    harness.send_key_with(KeyCode::Char('L'), KeyModifiers::SHIFT); // Toggle queued.
    assert!(
        harness
            .step_until(
                |b| buffer_to_string(b).contains("Q  "),
                Duration::from_secs(2),
            )
            .await,
        "queued item should render 'Q  ' marker"
    );

    harness.quit().await;
}

#[tokio::test]
async fn test_refresh_progress_visuals() {
    let mock = Arc::new(MockFetcher::with_delay(Duration::from_millis(200)));
    let ctx = Arc::new(AppContext::in_memory_with_fetcher(mock.clone()).unwrap());
    let feed_id = add_feed_with_items(&ctx, "alpha", 0);
    let feed = ctx.store.get_feed(feed_id).unwrap().unwrap();

    mock.set_response(
        feed.url.clone(),
        FetchResult::Content {
            body: br#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0"><channel><title>Alpha</title>
<item><title>New Item</title><link>http://example.com/1</link></item>
</channel></rss>"#
                .to_vec(),
            etag: None,
            last_modified: None,
        },
    );

    let mut harness = Harness::setup(ctx).await;

    harness.send_key_with(KeyCode::Char('R'), KeyModifiers::SHIFT);

    assert!(
        harness
            .step_until(
                |b| buffer_to_string(b).contains("Refreshing feeds..."),
                Duration::from_secs(5),
            )
            .await,
        "status bar should show refresh progress"
    );

    harness.quit().await;
}

#[tokio::test]
async fn test_toggle_actions_persist_through_dispatch() {
    // Drives Action::ToggleRead/Star/Queued/Saved/Archived via the actual
    // keybindings and verifies both the rendered buffer and the store reflect
    // each toggle. This is the coverage the unit-level test was claiming.
    let ctx = Arc::new(AppContext::in_memory().unwrap());
    let feed_id = add_feed_with_items(&ctx, "alpha", 1);
    let item_id = ctx.store.get_items_by_feed(feed_id).unwrap()[0].id.clone();
    let mut harness = Harness::setup(ctx.clone()).await;

    harness.send_key(KeyCode::Char(']')); // Reader.
    harness.send_key(KeyCode::Char('l')); // Focus Items.
    assert!(
        harness
            .step_until(
                |b| buffer_to_string(b).contains("alpha item 0"),
                Duration::from_secs(2),
            )
            .await,
        "items should load before toggling"
    );

    // Order matters: archive must be last because the default "All" view
    // filters out archived items, which would deselect the cursor.
    // Marker priority (per src/tui/layout.rs:319-329): archived > saved >
    // queued > starred > unread > read. We toggle in priority order so each
    // step actually changes the rendered marker.

    // Star: 's' → marker becomes "*  ".
    harness.send_key(KeyCode::Char('s'));
    assert!(
        harness
            .step_until(
                |b| buffer_to_string(b).contains("*  "),
                Duration::from_secs(2),
            )
            .await,
        "buffer should show starred marker"
    );
    assert!(
        ctx.store
            .get_item_state(&item_id)
            .unwrap()
            .unwrap()
            .is_starred
    );

    // Queued: Shift+l → marker becomes "Q  " (overrides star).
    harness.send_key_with(KeyCode::Char('L'), KeyModifiers::SHIFT);
    assert!(
        harness
            .step_until(
                |b| buffer_to_string(b).contains("Q  "),
                Duration::from_secs(2),
            )
            .await,
        "buffer should show queued marker"
    );
    assert!(
        ctx.store
            .get_item_state(&item_id)
            .unwrap()
            .unwrap()
            .is_queued
    );

    // Saved: Shift+s → marker becomes "S  " (overrides queue).
    harness.send_key_with(KeyCode::Char('S'), KeyModifiers::SHIFT);
    assert!(
        harness
            .step_until(
                |b| buffer_to_string(b).contains("S  "),
                Duration::from_secs(2),
            )
            .await,
        "buffer should show saved marker"
    );
    assert!(
        ctx.store
            .get_item_state(&item_id)
            .unwrap()
            .unwrap()
            .is_saved
    );

    // Read: 'r' → toggles is_read. Marker is blank when read, so we can't
    // assert via buffer; poll store. (Item still selected — saved items
    // remain in the All view.)
    harness.send_key(KeyCode::Char('r'));
    assert!(
        wait_for_store(&harness.tx, &ctx, &item_id, |s| s.is_read).await,
        "store should reflect read state"
    );

    // Archive last: 'x' removes the item from the All view, deselecting the
    // cursor — so any subsequent toggle would no-op. Poll store.
    harness.send_key(KeyCode::Char('x'));
    assert!(
        wait_for_store(&harness.tx, &ctx, &item_id, |s| s.is_archived).await,
        "store should reflect archived state"
    );

    harness.quit().await;
}

/// Poll the store every ~25ms for up to 2s, returning true once `predicate`
/// is satisfied. Used when the rendered marker is empty/ambiguous (read) or
/// the item leaves the view (archived).
async fn wait_for_store(
    _tx: &mpsc::UnboundedSender<AppEvent>,
    ctx: &AppContext,
    item_id: &str,
    predicate: impl Fn(&rivulet::domain::ItemState) -> bool,
) -> bool {
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(2) {
        if let Ok(Some(state)) = ctx.store.get_item_state(item_id) {
            if predicate(&state) {
                return true;
            }
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    false
}

#[tokio::test]
async fn test_new_marker_after_refresh() {
    // After a refresh batch, items inserted in that batch render with the
    // 'NEW' marker on the Latest tab.
    let mock = Arc::new(MockFetcher::new());
    let ctx = Arc::new(AppContext::in_memory_with_fetcher(mock.clone()).unwrap());
    let feed_id = add_feed_with_items(&ctx, "alpha", 0);
    let feed = ctx.store.get_feed(feed_id).unwrap().unwrap();

    mock.set_response(
        feed.url.clone(),
        FetchResult::Content {
            body: br#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0"><channel><title>Alpha</title>
<item><title>Brand New Item</title><link>http://example.com/new-1</link></item>
</channel></rss>"#
                .to_vec(),
            etag: None,
            last_modified: None,
        },
    );

    let mut harness = Harness::setup(ctx).await;

    harness.send_key_with(KeyCode::Char('R'), KeyModifiers::SHIFT); // Refresh.

    // Wait for the item to appear and the NEW marker to render.
    assert!(
        harness
            .step_until(
                |b| {
                    let s = buffer_to_string(b);
                    s.contains("NEW") && s.contains("Brand New Item")
                },
                Duration::from_secs(5),
            )
            .await,
        "Latest tab should render NEW marker for the just-refreshed item"
    );

    harness.quit().await;
}

#[tokio::test]
async fn test_quit_teardown() {
    let ctx = Arc::new(AppContext::in_memory().unwrap());
    let harness = Harness::setup(ctx).await;

    harness.send_key(KeyCode::Char('q'));

    tokio::time::timeout(Duration::from_secs(2), harness.join_handle)
        .await
        .expect("should quit within timeout")
        .expect("app should exit cleanly");
}

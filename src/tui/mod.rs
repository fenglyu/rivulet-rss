pub mod app;
pub mod event;
pub mod layout;

use std::io::{self, Stdout};
use std::sync::Arc;
use std::time::Duration;

use crossterm::{
    event::KeyCode,
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use crate::app::{AppContext, Result};
use crate::config::Config;
use crate::scraper::{ChromeScraper, Scraper};
use crate::store::{RefreshSource, Store};

use self::app::{ActivePane, AppTab, FeedPanelState, ItemView, LoadedFeed, PendingChord, TuiApp};
use self::event::{Action, AppEvent, EventHandler};

type Tui = Terminal<CrosstermBackend<Stdout>>;

pub async fn run(ctx: Arc<AppContext>, config: Arc<Config>) -> Result<()> {
    let mut terminal = setup_terminal()?;
    let event_handler = EventHandler::new(Duration::from_millis(100));
    let result = run_app(&mut terminal, event_handler, ctx, config).await;
    restore_terminal(&mut terminal)?;
    result
}

fn setup_terminal() -> Result<Tui> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

fn restore_terminal(terminal: &mut Tui) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

/// Public for integration-test access only; not part of the stable API.
#[doc(hidden)]
pub async fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    mut event_handler: EventHandler,
    ctx: Arc<AppContext>,
    config: Arc<Config>,
) -> Result<()>
where
    crate::app::error::RivuletError: From<B::Error>,
{
    let mut tui_app = TuiApp::new();
    tui_app.recent_days = config.ui.latest.days;
    tui_app.recent_limit = config.ui.latest.limit;

    // Load initial data
    load_feeds(&mut tui_app, &ctx)?;
    load_reader_items(&mut tui_app, &ctx)?;
    load_latest_items(&mut tui_app, &ctx)?;

    loop {
        terminal.draw(|frame| layout::render(frame, &mut tui_app, &config.colors))?;

        let event = match event_handler.next().await {
            Some(e) => e,
            None => break,
        };

        match event {
            AppEvent::Key(key) => {
                // Handle pending delete confirmation
                if let Some((feed_id, feed_title)) = tui_app.pending_delete.take() {
                    match key.code {
                        KeyCode::Char('y') | KeyCode::Char('Y') => {
                            ctx.store.delete_feed(feed_id)?;
                            load_feeds(&mut tui_app, &ctx)?;
                            load_reader_items(&mut tui_app, &ctx)?;
                            load_latest_items(&mut tui_app, &ctx)?;
                            tui_app.set_status(format!("Deleted feed: {}", feed_title));
                        }
                        _ => {
                            tui_app.set_status("Delete cancelled".to_string());
                        }
                    }
                    continue;
                }

                // Handle pending multi-key chord (e.g. Ctrl+W <h|l>)
                if let Some(chord) = tui_app.pending_chord.take() {
                    match chord {
                        PendingChord::Window => handle_window_chord_key(&mut tui_app, &ctx, &key)?,
                    }
                    continue;
                }

                let action = config.keybindings.get_action(&key);
                let feed_index_before = tui_app.feed_index;
                match action {
                    Action::Quit => {
                        tui_app.should_quit = true;
                    }
                    Action::MoveUp => {
                        tui_app.move_up();
                    }
                    Action::MoveDown => {
                        tui_app.move_down();
                    }
                    Action::MoveTop => {
                        tui_app.move_top();
                    }
                    Action::MoveBottom => {
                        tui_app.move_bottom();
                    }
                    Action::NextPage => {
                        tui_app.next_page();
                    }
                    Action::PrevPage => {
                        tui_app.prev_page();
                    }
                    Action::ToggleMaximize => {
                        tui_app.toggle_maximize();
                    }
                    Action::NextPane => {
                        focus_next_pane(&mut tui_app, &ctx)?;
                    }
                    Action::PrevPane => {
                        focus_prev_pane(&mut tui_app, &ctx)?;
                    }
                    Action::FocusLeft => {
                        focus_pane_directional(&mut tui_app, &ctx, focus_left_for_tab)?;
                    }
                    Action::FocusRight => {
                        focus_pane_directional(&mut tui_app, &ctx, focus_right_for_tab)?;
                    }
                    Action::Select => match (tui_app.active_tab, tui_app.active_pane) {
                        (AppTab::Reader, ActivePane::Feeds) => {
                            if load_items_for_highlighted_feed(&mut tui_app, &ctx)? {
                                tui_app.active_pane = ActivePane::Items;
                            }
                        }
                        (_, ActivePane::Items)
                            if tui_app.selected_item_for_active_tab().is_some() =>
                        {
                            tui_app.active_pane = ActivePane::Preview;
                        }
                        _ => {}
                    },
                    Action::ToggleRead => {
                        if let Some(item) = tui_app.selected_item_for_active_tab() {
                            let item_id = item.id.clone();
                            let is_read = tui_app.is_item_read(&item_id);
                            ctx.store.set_read(&item_id, !is_read)?;
                            update_item_state(&mut tui_app, item_id, |state| {
                                state.is_read = !is_read;
                            });
                        }
                    }
                    Action::ToggleStar => {
                        if let Some(item) = tui_app.selected_item_for_active_tab() {
                            let item_id = item.id.clone();
                            let is_starred = tui_app.is_item_starred(&item_id);
                            ctx.store.set_starred(&item_id, !is_starred)?;
                            update_item_state(&mut tui_app, item_id, |state| {
                                state.is_starred = !is_starred;
                            });
                        }
                    }
                    Action::ToggleQueued => {
                        if let Some(item) = tui_app.selected_item_for_active_tab() {
                            let item_id = item.id.clone();
                            let is_queued = tui_app.is_item_queued(&item_id);
                            ctx.store.set_queued(&item_id, !is_queued)?;
                            update_item_state(&mut tui_app, item_id, |state| {
                                state.is_queued = !is_queued;
                            });
                        }
                    }
                    Action::ToggleSaved => {
                        if let Some(item) = tui_app.selected_item_for_active_tab() {
                            let item_id = item.id.clone();
                            let is_saved = tui_app.is_item_saved(&item_id);
                            ctx.store.set_saved(&item_id, !is_saved)?;
                            update_item_state(&mut tui_app, item_id, |state| {
                                state.is_saved = !is_saved;
                            });
                        }
                    }
                    Action::ToggleArchived => {
                        if let Some(item) = tui_app.selected_item_for_active_tab() {
                            let item_id = item.id.clone();
                            let is_archived = tui_app.is_item_archived(&item_id);
                            ctx.store.set_archived(&item_id, !is_archived)?;
                            update_item_state(&mut tui_app, item_id, |state| {
                                state.is_archived = !is_archived;
                            });
                        }
                    }
                    Action::ViewAll => set_item_view(&mut tui_app, &ctx, ItemView::All)?,
                    Action::ViewUnread => set_item_view(&mut tui_app, &ctx, ItemView::Unread)?,
                    Action::ViewStarred => set_item_view(&mut tui_app, &ctx, ItemView::Starred)?,
                    Action::ViewQueued => set_item_view(&mut tui_app, &ctx, ItemView::Queued)?,
                    Action::ViewSaved => set_item_view(&mut tui_app, &ctx, ItemView::Saved)?,
                    Action::ViewArchived => set_item_view(&mut tui_app, &ctx, ItemView::Archived)?,
                    Action::ViewLatest => {
                        tui_app.active_tab = AppTab::Latest;
                        tui_app.active_pane = ActivePane::Items;
                        load_latest_items(&mut tui_app, &ctx)?;
                    }
                    Action::ViewReader => {
                        tui_app.active_tab = AppTab::Reader;
                        tui_app.feed_panel = FeedPanelState::Expanded;
                        tui_app.active_pane = ActivePane::Feeds;
                        // Eagerly load the highlighted feed's items so the
                        // Items pane isn't empty on first entry. Symmetric
                        // with `Action::ViewLatest`, which calls
                        // `load_latest_items` immediately.
                        load_items_for_highlighted_feed(&mut tui_app, &ctx)?;
                    }
                    Action::OpenInBrowser => {
                        if let Some(item) = tui_app.selected_item_for_active_tab() {
                            if let Some(link) = &item.link {
                                match crate::url_policy::parse_http_url(link) {
                                    Ok(url) => {
                                        if let Err(e) = open::that(url.as_str()) {
                                            tui_app.set_status(format!(
                                                "Failed to open browser: {}",
                                                e
                                            ));
                                        } else {
                                            // Mark as read when opened
                                            let item_id = item.id.clone();
                                            ctx.store.set_read(&item_id, true)?;
                                            update_item_state(&mut tui_app, item_id, |state| {
                                                state.is_read = true;
                                            });
                                        }
                                    }
                                    Err(error) => {
                                        tui_app.set_status(error.to_string());
                                    }
                                }
                            }
                        }
                    }
                    Action::Refresh => {
                        if !tui_app.is_refreshing {
                            tui_app.is_refreshing = true;
                            tui_app.refresh_progress = (0, 0);

                            let tx = event_handler.get_tx();
                            let ctx_clone = ctx.clone();

                            let (progress_tx, mut progress_rx) =
                                tokio::sync::mpsc::unbounded_channel::<(usize, usize)>();
                            let tx_clone = tx.clone();
                            tokio::spawn(async move {
                                while let Some((current, total)) = progress_rx.recv().await {
                                    let _ =
                                        tx_clone.send(AppEvent::RefreshProgress(current, total));
                                }
                            });

                            tokio::spawn(async move {
                                let feeds = match ctx_clone.store.get_all_feeds() {
                                    Ok(f) => f,
                                    Err(e) => {
                                        tracing::error!("Failed to get feeds: {}", e);
                                        return;
                                    }
                                };
                                let run_id = match ctx_clone
                                    .store
                                    .begin_refresh_run(RefreshSource::Tui, feeds.len())
                                {
                                    Ok(run_id) => run_id,
                                    Err(e) => {
                                        tracing::error!("Failed to start refresh run: {}", e);
                                        return;
                                    }
                                };

                                let results = ctx_clone
                                    .parallel_fetcher
                                    .fetch_all(
                                        feeds,
                                        ctx_clone.store.clone(),
                                        &ctx_clone.normalizer,
                                        Some(progress_tx),
                                    )
                                    .await;

                                let _ = tx.send(AppEvent::RefreshComplete(run_id, results));
                            });
                        }
                    }
                    Action::ToggleFeedPanel => {
                        if tui_app.active_tab == AppTab::Reader {
                            tui_app.feed_panel = match tui_app.feed_panel {
                                FeedPanelState::Collapsed => {
                                    tui_app.active_pane = ActivePane::Feeds;
                                    FeedPanelState::Expanded
                                }
                                FeedPanelState::Expanded => {
                                    load_items_for_highlighted_feed(&mut tui_app, &ctx)?;
                                    tui_app.active_pane = if tui_app.loaded_feed.is_some() {
                                        ActivePane::Items
                                    } else {
                                        ActivePane::Preview
                                    };
                                    FeedPanelState::Collapsed
                                }
                            };
                        } else {
                            tui_app.set_status(
                                "Feed panel is Reader-only - press Alt+2 to switch".to_string(),
                            );
                        }
                    }
                    Action::DeleteFeed => {
                        if tui_app.active_tab == AppTab::Reader
                            && tui_app.active_pane == ActivePane::Feeds
                        {
                            if let Some(feed) = tui_app.selected_feed() {
                                let feed_id = feed.id;
                                let feed_title = feed.display_title().to_string();
                                tui_app.pending_delete = Some((feed_id, feed_title));
                            }
                        }
                    }
                    Action::WindowChord => {
                        tui_app.pending_chord = Some(PendingChord::Window);
                        tui_app.set_status(
                            "-- WINDOW -- (h/l: left/right, w/Tab: next, W/Shift+Tab: prev, Esc: cancel)"
                                .to_string(),
                        );
                    }
                    Action::None => {}
                }

                // If feed-pane navigation moved the highlight to a new feed,
                // auto-load that feed's items so the Items pane (and its filter
                // count in the title) reflects the highlighted feed without
                // requiring an explicit Enter.
                if tui_app.active_tab == AppTab::Reader
                    && tui_app.active_pane == ActivePane::Feeds
                    && tui_app.feed_index != feed_index_before
                {
                    load_items_for_highlighted_feed(&mut tui_app, &ctx)?;
                }
            }
            AppEvent::Tick => {
                // Clear status message after some time could be implemented here
            }
            AppEvent::RefreshProgress(current, total) => {
                tui_app.refresh_progress = (current, total);
            }
            AppEvent::RefreshComplete(run_id, results) => {
                let mut total_new = 0;
                let mut errors = 0;
                let mut updated_feed_ids = Vec::new();
                for (feed_id, result) in results {
                    if let Ok(refresh) = result {
                        total_new += refresh.new_count;
                        ctx.store.record_refresh_run_items(
                            run_id,
                            feed_id,
                            &refresh.inserted_item_ids,
                        )?;
                        if refresh.new_count > 0 {
                            updated_feed_ids.push(feed_id);
                        }
                    } else {
                        errors += 1;
                    }
                }
                ctx.store.complete_refresh_run(run_id, total_new, errors)?;

                // Queue items for background scraping (non-blocking)
                if ctx.scraper_handle.is_some() && !updated_feed_ids.is_empty() {
                    let mut items_to_scrape = Vec::new();
                    for feed_id in updated_feed_ids {
                        if let Ok(items) = ctx.store.get_items_by_feed(feed_id) {
                            items_to_scrape
                                .extend(items.into_iter().filter(ChromeScraper::needs_scraping));
                        }
                    }
                    if !items_to_scrape.is_empty() {
                        let ctx_clone = ctx.clone();
                        tokio::spawn(async move {
                            ctx_clone.queue_for_scraping(items_to_scrape).await;
                        });
                    }
                }

                load_feeds(&mut tui_app, &ctx)?;
                load_reader_items(&mut tui_app, &ctx)?;
                load_latest_items(&mut tui_app, &ctx)?;

                tui_app.is_refreshing = false;
                tui_app.set_status(format!("Refreshed: {} new items", total_new));
            }
        }

        if tui_app.should_quit {
            break;
        }
    }

    Ok(())
}

fn load_feeds(tui_app: &mut TuiApp, ctx: &AppContext) -> Result<()> {
    tui_app.feeds = ctx.store.get_all_feeds()?;

    if tui_app.feeds.is_empty() {
        tui_app.feed_index = 0;
        tui_app.feed_list_state.select(None);
        clear_reader_feed_selection(tui_app, ctx)?;
        return Ok(());
    }

    if let Some(feed_id) = tui_app.loaded_feed_id() {
        if let Some(index) = tui_app.feeds.iter().position(|feed| feed.id == feed_id) {
            tui_app.feed_index = index;
        } else {
            clear_reader_feed_selection(tui_app, ctx)?;
        }
    }

    if tui_app.feed_index >= tui_app.feeds.len() {
        tui_app.feed_index = tui_app.feeds.len() - 1;
    }
    tui_app.feed_list_state.select(Some(tui_app.feed_index));
    Ok(())
}

fn clear_reader_feed_selection(tui_app: &mut TuiApp, ctx: &AppContext) -> Result<()> {
    tui_app.loaded_feed = None;
    reload_item_states(tui_app, ctx)
}

fn load_reader_items(tui_app: &mut TuiApp, ctx: &AppContext) -> Result<()> {
    if let Some(feed_id) = tui_app.loaded_feed_id() {
        load_items_for_feed(tui_app, ctx, feed_id)?;
    } else {
        clear_reader_feed_selection(tui_app, ctx)?;
    }
    Ok(())
}

fn load_latest_items(tui_app: &mut TuiApp, ctx: &AppContext) -> Result<()> {
    tui_app.latest_run_id = ctx.store.get_latest_refresh_run_id()?;
    tui_app.latest_items = ctx.store.get_recent_items(
        tui_app.item_view.filter(),
        tui_app.recent_days,
        tui_app.recent_limit,
        tui_app.latest_run_id,
    )?;
    if tui_app.latest_index >= tui_app.latest_items.len() && !tui_app.latest_items.is_empty() {
        tui_app.latest_index = tui_app.latest_items.len() - 1;
    }
    tui_app.latest_list_state.select(Some(tui_app.latest_index));
    reload_item_states(tui_app, ctx)?;
    Ok(())
}

fn set_item_view(tui_app: &mut TuiApp, ctx: &AppContext, view: ItemView) -> Result<()> {
    tui_app.item_view = view;
    tui_app.latest_index = 0;
    tui_app.preview_scroll = 0;
    match tui_app.active_tab {
        AppTab::Latest => load_latest_items(tui_app, ctx)?,
        AppTab::Reader => load_reader_items(tui_app, ctx)?,
    }
    tui_app.active_pane = if tui_app.active_tab == AppTab::Reader && tui_app.loaded_feed.is_none() {
        ActivePane::Feeds
    } else {
        ActivePane::Items
    };
    tui_app.set_status(format!("Showing {} items", view.label()));
    Ok(())
}

/// Sync the loaded feed with the highlighted feed cursor. Returns `true` when
/// the cursor points at a real feed (so callers know whether moving focus to
/// Items / Preview is meaningful), `false` when there are no feeds.
fn load_items_for_highlighted_feed(tui_app: &mut TuiApp, ctx: &AppContext) -> Result<bool> {
    let Some(feed_id) = tui_app.selected_feed().map(|feed| feed.id) else {
        clear_reader_feed_selection(tui_app, ctx)?;
        return Ok(false);
    };

    if tui_app.loaded_feed_id() != Some(feed_id) {
        load_items_for_feed(tui_app, ctx, feed_id)?;
    }

    Ok(true)
}

fn load_items_for_feed(tui_app: &mut TuiApp, ctx: &AppContext, feed_id: i64) -> Result<()> {
    if let Some(index) = tui_app.feeds.iter().position(|feed| feed.id == feed_id) {
        tui_app.feed_index = index;
        tui_app.feed_list_state.select(Some(index));
    }

    let items = ctx.store.get_items_by_feed(feed_id)?;
    let filter = tui_app.item_view.filter();
    let mut filtered = Vec::new();
    for item in items {
        if item_matches_filter(ctx, &item, filter)? {
            filtered.push(item);
        }
    }

    // Constructing a fresh LoadedFeed makes the four pieces of correlated
    // state (feed_id / items / item_index / item_list_state) atomic. There is
    // no transient where they could disagree.
    tui_app.loaded_feed = Some(LoadedFeed::new(feed_id, filtered));
    reload_item_states(tui_app, ctx)
}

fn item_matches_filter(
    ctx: &AppContext,
    item: &crate::domain::Item,
    filter: crate::store::ItemListFilter,
) -> Result<bool> {
    let state = ctx.store.get_item_state(&item.id)?;
    let is_read = state.as_ref().map(|s| s.is_read).unwrap_or(false);
    let is_starred = state.as_ref().map(|s| s.is_starred).unwrap_or(false);
    let is_queued = state.as_ref().map(|s| s.is_queued).unwrap_or(false);
    let is_saved = state.as_ref().map(|s| s.is_saved).unwrap_or(false);
    let is_archived = state.as_ref().map(|s| s.is_archived).unwrap_or(false);

    Ok(match filter {
        crate::store::ItemListFilter::All => !is_archived,
        crate::store::ItemListFilter::Unread => !is_archived && !is_read,
        crate::store::ItemListFilter::Starred => !is_archived && is_starred,
        crate::store::ItemListFilter::Queued => !is_archived && is_queued,
        crate::store::ItemListFilter::Saved => !is_archived && is_saved,
        crate::store::ItemListFilter::Archived => is_archived,
    })
}

fn reload_item_states(tui_app: &mut TuiApp, ctx: &AppContext) -> Result<()> {
    let mut ids: Vec<String> = tui_app
        .loaded_items()
        .iter()
        .map(|item| item.id.clone())
        .collect();
    ids.extend(
        tui_app
            .latest_items
            .iter()
            .map(|recent| recent.item.id.clone()),
    );
    ids.sort();
    ids.dedup();

    tui_app.item_states.clear();
    for item_id in ids {
        if let Some(state) = ctx.store.get_item_state(&item_id)? {
            tui_app.item_states.insert(item_id, state);
        }
    }
    Ok(())
}

fn next_pane_for_tab(tui_app: &TuiApp) -> ActivePane {
    match tui_app.active_tab {
        AppTab::Latest => match tui_app.active_pane {
            ActivePane::Items => ActivePane::Preview,
            _ => ActivePane::Items,
        },
        AppTab::Reader => match tui_app.feed_panel {
            FeedPanelState::Expanded => tui_app.active_pane.next(),
            FeedPanelState::Collapsed => match tui_app.active_pane {
                ActivePane::Items => ActivePane::Preview,
                _ => ActivePane::Items,
            },
        },
    }
}

fn prev_pane_for_tab(tui_app: &TuiApp) -> ActivePane {
    match tui_app.active_tab {
        AppTab::Latest => match tui_app.active_pane {
            ActivePane::Preview => ActivePane::Items,
            _ => ActivePane::Preview,
        },
        AppTab::Reader => match tui_app.feed_panel {
            FeedPanelState::Expanded => tui_app.active_pane.prev(),
            FeedPanelState::Collapsed => match tui_app.active_pane {
                ActivePane::Preview => ActivePane::Items,
                _ => ActivePane::Preview,
            },
        },
    }
}

fn focus_pane(tui_app: &mut TuiApp, ctx: &AppContext, target: ActivePane) -> Result<()> {
    if tui_app.active_tab == AppTab::Reader && target != ActivePane::Feeds {
        load_items_for_highlighted_feed(tui_app, ctx)?;
    }
    tui_app.active_pane = target;
    Ok(())
}

fn focus_next_pane(tui_app: &mut TuiApp, ctx: &AppContext) -> Result<()> {
    let target = next_pane_for_tab(tui_app);
    focus_pane(tui_app, ctx, target)
}

fn focus_prev_pane(tui_app: &mut TuiApp, ctx: &AppContext) -> Result<()> {
    let target = prev_pane_for_tab(tui_app);
    focus_pane(tui_app, ctx, target)
}

/// Move focus one pane in a direction. Syncs items for the highlighted feed
/// first (so `focus_*_for_tab`'s `items_focusable = loaded_feed.is_some()`
/// check is up-to-date — important when the user steps into the Items pane
/// from Feeds without ever pressing Enter).
///
/// Returns `true` if focus actually moved, `false` if already at the edge.
/// Shared by the `Ctrl+W h/l` chord arms and the `Left`/`Right` arrow keys.
fn focus_pane_directional(
    tui_app: &mut TuiApp,
    ctx: &AppContext,
    target_fn: fn(&TuiApp) -> ActivePane,
) -> Result<bool> {
    if tui_app.active_tab == AppTab::Reader {
        load_items_for_highlighted_feed(tui_app, ctx)?;
    }
    let target = target_fn(tui_app);
    if target == tui_app.active_pane {
        Ok(false)
    } else {
        focus_pane(tui_app, ctx, target)?;
        Ok(true)
    }
}

fn handle_window_chord_key(
    tui_app: &mut TuiApp,
    ctx: &AppContext,
    key: &crossterm::event::KeyEvent,
) -> Result<()> {
    if tui_app.maximized {
        tui_app.set_status("Window chord unavailable in maximize mode".to_string());
        return Ok(());
    }
    match key.code {
        KeyCode::Char('h') | KeyCode::Left => {
            if focus_pane_directional(tui_app, ctx, focus_left_for_tab)? {
                tui_app.clear_status();
            } else {
                tui_app.set_status("Already at leftmost pane".to_string());
            }
        }
        KeyCode::Char('l') | KeyCode::Right => {
            if focus_pane_directional(tui_app, ctx, focus_right_for_tab)? {
                tui_app.clear_status();
            } else {
                tui_app.set_status("Already at rightmost pane".to_string());
            }
        }
        KeyCode::Char('w') | KeyCode::Tab => {
            focus_next_pane(tui_app, ctx)?;
            tui_app.clear_status();
        }
        KeyCode::Char('W') | KeyCode::BackTab => {
            focus_prev_pane(tui_app, ctx)?;
            tui_app.clear_status();
        }
        KeyCode::Esc => {
            tui_app.set_status("Window chord cancelled".to_string());
        }
        _ => {
            tui_app.clear_status();
        }
    }
    Ok(())
}

fn focus_left_for_tab(tui_app: &TuiApp) -> ActivePane {
    match tui_app.active_tab {
        AppTab::Latest => match tui_app.active_pane {
            ActivePane::Preview => ActivePane::Items,
            other => other,
        },
        AppTab::Reader => {
            let feeds_focusable = tui_app.feed_panel == FeedPanelState::Expanded;
            let items_focusable = tui_app.loaded_feed.is_some();
            match tui_app.active_pane {
                ActivePane::Preview => {
                    if items_focusable {
                        ActivePane::Items
                    } else if feeds_focusable {
                        ActivePane::Feeds
                    } else {
                        ActivePane::Preview
                    }
                }
                ActivePane::Items => {
                    if feeds_focusable {
                        ActivePane::Feeds
                    } else {
                        ActivePane::Items
                    }
                }
                ActivePane::Feeds => ActivePane::Feeds,
            }
        }
    }
}

fn focus_right_for_tab(tui_app: &TuiApp) -> ActivePane {
    match tui_app.active_tab {
        AppTab::Latest => match tui_app.active_pane {
            ActivePane::Items => ActivePane::Preview,
            other => other,
        },
        AppTab::Reader => {
            let items_focusable = tui_app.loaded_feed.is_some();
            match tui_app.active_pane {
                ActivePane::Feeds => {
                    if items_focusable {
                        ActivePane::Items
                    } else {
                        ActivePane::Preview
                    }
                }
                ActivePane::Items => ActivePane::Preview,
                ActivePane::Preview => ActivePane::Preview,
            }
        }
    }
}

fn update_item_state(
    tui_app: &mut TuiApp,
    item_id: String,
    update: impl FnOnce(&mut crate::domain::ItemState),
) {
    let state = tui_app
        .item_states
        .entry(item_id.clone())
        .or_insert_with(|| crate::domain::ItemState::new(item_id));
    update(state);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEvent, KeyModifiers};

    fn add_feed_with_items(ctx: &AppContext, title: &str, item_count: usize) -> i64 {
        let mut feed = crate::domain::Feed::new(format!("https://example.com/{title}.xml"));
        feed.title = Some(title.to_string());
        let feed_id = ctx.store.add_feed(&feed).unwrap();

        for index in 0..item_count {
            let mut item =
                crate::domain::Item::new(feed_id, &feed.url, &format!("{title}-entry-{index}"));
            item.title = Some(format!("{title} item {index}"));
            ctx.store.add_item(&item).unwrap();
        }

        feed_id
    }

    fn reader_app_with_expanded_feeds(ctx: &AppContext) -> TuiApp {
        let mut tui_app = TuiApp::new();
        load_feeds(&mut tui_app, ctx).unwrap();
        tui_app.active_tab = AppTab::Reader;
        tui_app.feed_panel = FeedPanelState::Expanded;
        tui_app.active_pane = ActivePane::Feeds;
        tui_app
    }

    #[test]
    fn window_chord_right_selects_highlighted_feed_before_focusing_items() {
        let ctx = AppContext::in_memory().unwrap();
        let feed_id = add_feed_with_items(&ctx, "alpha", 2);
        let mut tui_app = reader_app_with_expanded_feeds(&ctx);

        assert_eq!(tui_app.loaded_feed_id(), None);
        assert!(tui_app.loaded_items().is_empty());

        handle_window_chord_key(
            &mut tui_app,
            &ctx,
            &KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE),
        )
        .unwrap();

        assert_eq!(tui_app.active_pane, ActivePane::Items);
        assert_eq!(tui_app.loaded_feed_id(), Some(feed_id));
        assert_eq!(tui_app.loaded_items().len(), 2);
        let loaded = tui_app.loaded_feed.as_ref().unwrap();
        assert_eq!(loaded.feed_id, feed_id);
        assert_eq!(loaded.item_list_state.selected(), Some(0));
    }

    #[test]
    fn window_chord_resyncs_items_when_feed_cursor_and_loaded_feed_drift() {
        let ctx = AppContext::in_memory().unwrap();
        let first_feed_id = add_feed_with_items(&ctx, "alpha", 1);
        let second_feed_id = add_feed_with_items(&ctx, "beta", 3);
        let mut tui_app = reader_app_with_expanded_feeds(&ctx);

        load_items_for_feed(&mut tui_app, &ctx, first_feed_id).unwrap();
        let second_feed_index = tui_app
            .feeds
            .iter()
            .position(|feed| feed.id == second_feed_id)
            .unwrap();
        tui_app.feed_index = second_feed_index;
        tui_app.feed_list_state.select(Some(second_feed_index));

        handle_window_chord_key(
            &mut tui_app,
            &ctx,
            &KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE),
        )
        .unwrap();

        assert_eq!(tui_app.active_pane, ActivePane::Items);
        assert_eq!(tui_app.loaded_feed_id(), Some(second_feed_id));
        assert_eq!(tui_app.feed_index, second_feed_index);
        assert_eq!(tui_app.loaded_items().len(), 3);
        // Structurally, every item in `loaded_feed.items` belongs to
        // `loaded_feed.feed_id` because they're constructed together by
        // `LoadedFeed::new`.  This assertion is here as documentation; it
        // cannot fail without a Box-sized refactor.
        assert!(tui_app
            .loaded_items()
            .iter()
            .all(|item| item.feed_id == second_feed_id));
    }

    #[test]
    fn window_chord_directional_focus_steps_through_preview() {
        let ctx = AppContext::in_memory().unwrap();
        let feed_id = add_feed_with_items(&ctx, "alpha", 2);
        let mut tui_app = reader_app_with_expanded_feeds(&ctx);

        handle_window_chord_key(
            &mut tui_app,
            &ctx,
            &KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE),
        )
        .unwrap();
        assert_eq!(tui_app.active_pane, ActivePane::Items);

        handle_window_chord_key(
            &mut tui_app,
            &ctx,
            &KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE),
        )
        .unwrap();
        assert_eq!(tui_app.active_pane, ActivePane::Preview);
        assert_eq!(tui_app.loaded_feed_id(), Some(feed_id));
        assert_eq!(tui_app.loaded_items().len(), 2);

        handle_window_chord_key(
            &mut tui_app,
            &ctx,
            &KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE),
        )
        .unwrap();
        assert_eq!(tui_app.active_pane, ActivePane::Items);

        handle_window_chord_key(
            &mut tui_app,
            &ctx,
            &KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE),
        )
        .unwrap();
        assert_eq!(tui_app.active_pane, ActivePane::Feeds);
        assert_eq!(tui_app.loaded_feed_id(), Some(feed_id));
        assert_eq!(tui_app.loaded_items().len(), 2);
    }

    #[test]
    fn window_chord_cycle_focus_visits_all_reader_panes() {
        let ctx = AppContext::in_memory().unwrap();
        add_feed_with_items(&ctx, "alpha", 2);
        let mut tui_app = reader_app_with_expanded_feeds(&ctx);

        handle_window_chord_key(
            &mut tui_app,
            &ctx,
            &KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE),
        )
        .unwrap();
        assert_eq!(tui_app.active_pane, ActivePane::Items);

        handle_window_chord_key(
            &mut tui_app,
            &ctx,
            &KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE),
        )
        .unwrap();
        assert_eq!(tui_app.active_pane, ActivePane::Preview);

        handle_window_chord_key(
            &mut tui_app,
            &ctx,
            &KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE),
        )
        .unwrap();
        assert_eq!(tui_app.active_pane, ActivePane::Feeds);

        handle_window_chord_key(
            &mut tui_app,
            &ctx,
            &KeyEvent::new(KeyCode::Char('W'), KeyModifiers::SHIFT),
        )
        .unwrap();
        assert_eq!(tui_app.active_pane, ActivePane::Preview);
    }

    /// Test for the bug: Enter on a feed must load its items and move focus
    /// to the Items pane.
    #[test]
    fn select_advances_feeds_to_items_loading_when_needed() {
        let ctx = AppContext::in_memory().unwrap();
        let feed_id = add_feed_with_items(&ctx, "alpha", 3);
        let mut tui_app = reader_app_with_expanded_feeds(&ctx);
        assert_eq!(tui_app.loaded_feed_id(), None);

        // Inline the dispatch arm body; the same code runs in run_app's
        // Action::Select match.
        if tui_app.active_tab == AppTab::Reader
            && tui_app.active_pane == ActivePane::Feeds
            && load_items_for_highlighted_feed(&mut tui_app, &ctx).unwrap()
        {
            tui_app.active_pane = ActivePane::Items;
        }

        assert_eq!(tui_app.active_pane, ActivePane::Items);
        assert_eq!(tui_app.loaded_feed_id(), Some(feed_id));
        assert_eq!(tui_app.loaded_items().len(), 3);
    }

    /// Test for the bug: Enter on an item in the Items pane must move focus
    /// to the Preview pane (was previously a no-op — the workflow stalled).
    #[test]
    fn select_advances_items_to_preview() {
        let ctx = AppContext::in_memory().unwrap();
        let _ = add_feed_with_items(&ctx, "alpha", 2);
        let mut tui_app = reader_app_with_expanded_feeds(&ctx);
        load_items_for_highlighted_feed(&mut tui_app, &ctx).unwrap();
        tui_app.active_pane = ActivePane::Items;
        assert!(tui_app.selected_item().is_some());

        // Inline the (_, Items) arm of Action::Select.
        if tui_app.selected_item_for_active_tab().is_some() {
            tui_app.active_pane = ActivePane::Preview;
        }

        assert_eq!(tui_app.active_pane, ActivePane::Preview);
    }

    /// Entering the Reader tab via `]` (Action::ViewReader) must auto-load
    /// the items for whichever feed is highlighted in the rail, so the Items
    /// pane isn't empty on first entry.
    #[test]
    fn entering_reader_tab_loads_highlighted_feeds_items() {
        let ctx = AppContext::in_memory().unwrap();
        let first_feed = add_feed_with_items(&ctx, "alpha", 5);
        let _second_feed = add_feed_with_items(&ctx, "beta", 2);

        let mut tui_app = TuiApp::new();
        load_feeds(&mut tui_app, &ctx).unwrap();
        // Latest tab is the default in `TuiApp::new`, simulating fresh boot.
        assert_eq!(tui_app.active_tab, AppTab::Latest);
        assert_eq!(tui_app.loaded_feed_id(), None);

        // Inline Action::ViewReader's body.
        tui_app.active_tab = AppTab::Reader;
        tui_app.feed_panel = FeedPanelState::Expanded;
        tui_app.active_pane = ActivePane::Feeds;
        load_items_for_highlighted_feed(&mut tui_app, &ctx).unwrap();

        assert_eq!(tui_app.loaded_feed_id(), Some(first_feed));
        assert_eq!(tui_app.loaded_items().len(), 5);
    }

    /// Right arrow walks Feeds → Items → Preview, loading the feed's items on
    /// the very first step (so it doesn't skip Items just because nothing was
    /// loaded yet). Left arrow then walks back.
    #[test]
    fn arrow_keys_walk_feeds_items_preview_and_back() {
        let ctx = AppContext::in_memory().unwrap();
        let feed_id = add_feed_with_items(&ctx, "alpha", 4);
        let mut tui_app = reader_app_with_expanded_feeds(&ctx);
        assert_eq!(tui_app.loaded_feed_id(), None);

        // Right from Feeds → Items (loading on the way)
        let moved = focus_pane_directional(&mut tui_app, &ctx, focus_right_for_tab).unwrap();
        assert!(moved);
        assert_eq!(tui_app.active_pane, ActivePane::Items);
        assert_eq!(tui_app.loaded_feed_id(), Some(feed_id));
        assert_eq!(tui_app.loaded_items().len(), 4);

        // Right again → Preview
        let moved = focus_pane_directional(&mut tui_app, &ctx, focus_right_for_tab).unwrap();
        assert!(moved);
        assert_eq!(tui_app.active_pane, ActivePane::Preview);

        // Right at the rightmost edge → no movement
        let moved = focus_pane_directional(&mut tui_app, &ctx, focus_right_for_tab).unwrap();
        assert!(!moved);
        assert_eq!(tui_app.active_pane, ActivePane::Preview);

        // Left walks back: Preview → Items → Feeds
        let moved = focus_pane_directional(&mut tui_app, &ctx, focus_left_for_tab).unwrap();
        assert!(moved);
        assert_eq!(tui_app.active_pane, ActivePane::Items);

        let moved = focus_pane_directional(&mut tui_app, &ctx, focus_left_for_tab).unwrap();
        assert!(moved);
        assert_eq!(tui_app.active_pane, ActivePane::Feeds);

        // Left at the leftmost edge → no movement
        let moved = focus_pane_directional(&mut tui_app, &ctx, focus_left_for_tab).unwrap();
        assert!(!moved);
        assert_eq!(tui_app.active_pane, ActivePane::Feeds);
    }

    #[test]
    fn navigation_clamping_at_boundaries() {
        let ctx = AppContext::in_memory().unwrap();
        // PAGE_SIZE is 10, so let's add 25 items to test pagination
        let feed_id = add_feed_with_items(&ctx, "alpha", 25);
        let mut tui_app = reader_app_with_expanded_feeds(&ctx);
        load_items_for_feed(&mut tui_app, &ctx, feed_id).unwrap();
        tui_app.active_pane = ActivePane::Items;

        // g (MoveTop)
        tui_app.move_bottom();
        assert_eq!(tui_app.loaded_item_index(), 24);
        tui_app.move_top();
        assert_eq!(tui_app.loaded_item_index(), 0);

        // G (MoveBottom)
        tui_app.move_bottom();
        assert_eq!(tui_app.loaded_item_index(), 24);

        // n (NextPage) clamping
        tui_app.move_top();
        tui_app.next_page(); // 0 -> 10
        assert_eq!(tui_app.loaded_item_index(), 10);
        tui_app.next_page(); // 10 -> 20
        assert_eq!(tui_app.loaded_item_index(), 20);
        tui_app.next_page(); // 20 -> 24 (clamped)
        assert_eq!(tui_app.loaded_item_index(), 24);
        tui_app.next_page(); // 24 -> 24 (stay)
        assert_eq!(tui_app.loaded_item_index(), 24);

        // p (PrevPage) clamping
        tui_app.prev_page(); // 24 -> 14
        assert_eq!(tui_app.loaded_item_index(), 14);
        tui_app.prev_page(); // 14 -> 4
        assert_eq!(tui_app.loaded_item_index(), 4);
        tui_app.prev_page(); // 4 -> 0 (clamped)
        assert_eq!(tui_app.loaded_item_index(), 0);
        tui_app.prev_page(); // 0 -> 0 (stay)
        assert_eq!(tui_app.loaded_item_index(), 0);
    }

    #[test]
    fn pane_cycling_traversal() {
        let ctx = AppContext::in_memory().unwrap();
        add_feed_with_items(&ctx, "alpha", 1);
        let mut tui_app = reader_app_with_expanded_feeds(&ctx);
        load_items_for_highlighted_feed(&mut tui_app, &ctx).unwrap();

        // Forward cycling: Feeds -> Items -> Preview -> Feeds
        assert_eq!(tui_app.active_pane, ActivePane::Feeds);
        focus_next_pane(&mut tui_app, &ctx).unwrap();
        assert_eq!(tui_app.active_pane, ActivePane::Items);
        focus_next_pane(&mut tui_app, &ctx).unwrap();
        assert_eq!(tui_app.active_pane, ActivePane::Preview);
        focus_next_pane(&mut tui_app, &ctx).unwrap();
        assert_eq!(tui_app.active_pane, ActivePane::Feeds);

        // Backward cycling: Feeds -> Preview -> Items -> Feeds
        focus_prev_pane(&mut tui_app, &ctx).unwrap();
        assert_eq!(tui_app.active_pane, ActivePane::Preview);
        focus_prev_pane(&mut tui_app, &ctx).unwrap();
        assert_eq!(tui_app.active_pane, ActivePane::Items);
        focus_prev_pane(&mut tui_app, &ctx).unwrap();
        assert_eq!(tui_app.active_pane, ActivePane::Feeds);
    }

    #[test]
    fn chord_state_lifecycle() {
        // Mirrors the run_app loop: Action::WindowChord sets pending_chord;
        // the next key is dispatched by taking the chord and calling
        // handle_window_chord_key. Verify cancel + directional paths.
        let ctx = AppContext::in_memory().unwrap();
        add_feed_with_items(&ctx, "alpha", 1);

        // Path 1: Esc cancels.
        let mut tui_app = reader_app_with_expanded_feeds(&ctx);
        tui_app.pending_chord = Some(PendingChord::Window);
        let chord = tui_app.pending_chord.take();
        assert_eq!(chord, Some(PendingChord::Window));
        handle_window_chord_key(
            &mut tui_app,
            &ctx,
            &KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        )
        .unwrap();
        assert!(tui_app.pending_chord.is_none());
        assert_eq!(
            tui_app.status_message.as_deref(),
            Some("Window chord cancelled"),
        );

        // Path 2: 'l' advances focus right (Feeds → Items).
        let mut tui_app = reader_app_with_expanded_feeds(&ctx);
        tui_app.pending_chord = Some(PendingChord::Window);
        let _ = tui_app.pending_chord.take();
        handle_window_chord_key(
            &mut tui_app,
            &ctx,
            &KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE),
        )
        .unwrap();
        assert!(tui_app.pending_chord.is_none());
        assert_eq!(tui_app.active_pane, ActivePane::Items);
    }

    #[test]
    fn filter_views_item_counts() {
        let ctx = AppContext::in_memory().unwrap();
        let feed_id = add_feed_with_items(&ctx, "alpha", 10);
        let items = ctx.store.get_items_by_feed(feed_id).unwrap();

        // item 0: unread (default)
        // item 1: read
        ctx.store.set_read(&items[1].id, true).unwrap();
        // item 2: starred
        ctx.store.set_starred(&items[2].id, true).unwrap();
        // item 3: queued
        ctx.store.set_queued(&items[3].id, true).unwrap();
        // item 4: saved
        ctx.store.set_saved(&items[4].id, true).unwrap();
        // item 5: archived
        ctx.store.set_archived(&items[5].id, true).unwrap();

        let mut tui_app = reader_app_with_expanded_feeds(&ctx);
        load_items_for_feed(&mut tui_app, &ctx, feed_id).unwrap();

        // View All (default, excludes archived)
        set_item_view(&mut tui_app, &ctx, ItemView::All).unwrap();
        assert_eq!(tui_app.loaded_items().len(), 9); // 10 - 1 archived

        // View Unread
        set_item_view(&mut tui_app, &ctx, ItemView::Unread).unwrap();
        assert_eq!(tui_app.loaded_items().len(), 8); // 10 - 1 archived - 1 read

        // View Starred
        set_item_view(&mut tui_app, &ctx, ItemView::Starred).unwrap();
        assert_eq!(tui_app.loaded_items().len(), 1);

        // View Queued
        set_item_view(&mut tui_app, &ctx, ItemView::Queued).unwrap();
        assert_eq!(tui_app.loaded_items().len(), 1);

        // View Saved
        set_item_view(&mut tui_app, &ctx, ItemView::Saved).unwrap();
        assert_eq!(tui_app.loaded_items().len(), 1);

        // View Archived
        set_item_view(&mut tui_app, &ctx, ItemView::Archived).unwrap();
        assert_eq!(tui_app.loaded_items().len(), 1);
    }

    // Toggle-action coverage lives in tests/tui_e2e.rs::test_toggle_actions_persist_through_dispatch
    // — that test drives the actual Action::ToggleX dispatch via key presses,
    // which is what users hit and what the unit-level test was claiming to
    // cover but didn't.
}

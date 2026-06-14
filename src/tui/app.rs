use ratatui::widgets::ListState;

use crate::domain::{Feed, Item, ItemState};
use crate::store::{ItemListFilter, RecentItem};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivePane {
    Feeds,
    Items,
    Preview,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppTab {
    Latest,
    Reader,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedPanelState {
    Collapsed,
    Expanded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingChord {
    Window,
}

impl ActivePane {
    pub fn next(self) -> Self {
        match self {
            ActivePane::Feeds => ActivePane::Items,
            ActivePane::Items => ActivePane::Preview,
            ActivePane::Preview => ActivePane::Feeds,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            ActivePane::Feeds => ActivePane::Preview,
            ActivePane::Items => ActivePane::Feeds,
            ActivePane::Preview => ActivePane::Items,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemView {
    All,
    Unread,
    Starred,
    Queued,
    Saved,
    Archived,
}

impl ItemView {
    pub fn filter(self) -> ItemListFilter {
        match self {
            ItemView::All => ItemListFilter::All,
            ItemView::Unread => ItemListFilter::Unread,
            ItemView::Starred => ItemListFilter::Starred,
            ItemView::Queued => ItemListFilter::Queued,
            ItemView::Saved => ItemListFilter::Saved,
            ItemView::Archived => ItemListFilter::Archived,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ItemView::All => "All",
            ItemView::Unread => "Unread",
            ItemView::Starred => "Starred",
            ItemView::Queued => "Queue",
            ItemView::Saved => "Saved",
            ItemView::Archived => "Archived",
        }
    }
}

pub const PAGE_SIZE: usize = 10;

/// All four pieces of state that describe "items for the loaded Reader feed are
/// rendered." Bundling them makes the invariant structural: you cannot have an
/// `items` Vec without a `feed_id`, and you cannot have an `item_index` /
/// `item_list_state` referring to a different list than the one that's loaded.
///
/// Construct via [`LoadedFeed::new`]; mutate the cursor only through the
/// methods on this type so `item_index` and `item_list_state` cannot drift.
#[derive(Debug)]
pub struct LoadedFeed {
    pub feed_id: i64,
    pub items: Vec<Item>,
    pub item_index: usize,
    pub item_list_state: ListState,
}

impl LoadedFeed {
    pub fn new(feed_id: i64, items: Vec<Item>) -> Self {
        let mut state = ListState::default();
        if !items.is_empty() {
            state.select(Some(0));
        }
        Self {
            feed_id,
            items,
            item_index: 0,
            item_list_state: state,
        }
    }

    pub fn selected_item(&self) -> Option<&Item> {
        self.items.get(self.item_index)
    }

    /// Set the highlighted item index, clamped to the items list. Returns
    /// `true` if the cursor actually moved (callers use this to know whether
    /// preview-scroll should reset).
    pub fn select(&mut self, index: usize) -> bool {
        if self.items.is_empty() {
            self.item_index = 0;
            self.item_list_state.select(None);
            return false;
        }
        let bounded = index.min(self.items.len() - 1);
        if bounded == self.item_index && self.item_list_state.selected() == Some(bounded) {
            return false;
        }
        self.item_index = bounded;
        self.item_list_state.select(Some(bounded));
        true
    }

    pub fn move_up(&mut self) -> bool {
        if self.item_index == 0 {
            return false;
        }
        self.select(self.item_index - 1)
    }

    pub fn move_down(&mut self) -> bool {
        if self.items.is_empty() || self.item_index + 1 >= self.items.len() {
            return false;
        }
        self.select(self.item_index + 1)
    }

    pub fn move_top(&mut self) -> bool {
        self.select(0)
    }

    pub fn move_bottom(&mut self) -> bool {
        if self.items.is_empty() {
            return false;
        }
        self.select(self.items.len() - 1)
    }

    pub fn next_page(&mut self) -> bool {
        let max_index = self.items.len().saturating_sub(1);
        self.select((self.item_index + PAGE_SIZE).min(max_index))
    }

    pub fn prev_page(&mut self) -> bool {
        self.select(self.item_index.saturating_sub(PAGE_SIZE))
    }

    /// Re-clamp the cursor and `ListState` after `items` was replaced
    /// externally (e.g. filtered by view change).
    pub fn reclamp_cursor(&mut self) {
        if self.items.is_empty() {
            self.item_index = 0;
            self.item_list_state.select(None);
        } else {
            if self.item_index >= self.items.len() {
                self.item_index = self.items.len() - 1;
            }
            self.item_list_state.select(Some(self.item_index));
        }
    }
}

pub struct TuiApp {
    pub active_tab: AppTab,
    pub active_pane: ActivePane,
    pub feed_panel: FeedPanelState,
    pub feeds: Vec<Feed>,
    pub latest_items: Vec<RecentItem>,
    /// `Some` iff a feed has been loaded into the Reader Items pane. The four
    /// pieces of correlated state (feed_id / items / item_index /
    /// item_list_state) live together inside `LoadedFeed` so they cannot
    /// drift.
    pub loaded_feed: Option<LoadedFeed>,
    pub item_states: std::collections::HashMap<String, ItemState>,
    pub feed_index: usize,
    pub latest_index: usize,
    pub item_view: ItemView,
    pub preview_scroll: u16,
    pub should_quit: bool,
    pub status_message: Option<String>,
    pub is_refreshing: bool,
    pub refresh_progress: (usize, usize),
    // Maximize mode
    pub maximized: bool,
    // List states for scrolling
    pub feed_list_state: ListState,
    pub latest_list_state: ListState,
    pub latest_run_id: Option<i64>,
    pub recent_days: u32,
    pub recent_limit: usize,
    // Pending delete confirmation (feed_id, feed_title)
    pub pending_delete: Option<(i64, String)>,
    // Pending multi-key chord (e.g. Ctrl+W awaiting a direction)
    pub pending_chord: Option<PendingChord>,
}

impl TuiApp {
    pub fn new() -> Self {
        let mut feed_list_state = ListState::default();
        feed_list_state.select(Some(0));
        let mut latest_list_state = ListState::default();
        latest_list_state.select(Some(0));

        Self {
            active_tab: AppTab::Latest,
            active_pane: ActivePane::Items,
            feed_panel: FeedPanelState::Collapsed,
            feeds: Vec::new(),
            latest_items: Vec::new(),
            loaded_feed: None,
            item_states: std::collections::HashMap::new(),
            feed_index: 0,
            latest_index: 0,
            item_view: ItemView::All,
            preview_scroll: 0,
            should_quit: false,
            status_message: None,
            is_refreshing: false,
            refresh_progress: (0, 0),
            maximized: false,
            feed_list_state,
            latest_list_state,
            latest_run_id: None,
            recent_days: 7,
            recent_limit: 200,
            pending_delete: None,
            pending_chord: None,
        }
    }

    pub fn selected_feed(&self) -> Option<&Feed> {
        self.feeds.get(self.feed_index)
    }

    pub fn loaded_feed_id(&self) -> Option<i64> {
        self.loaded_feed.as_ref().map(|loaded| loaded.feed_id)
    }

    pub fn loaded_items(&self) -> &[Item] {
        self.loaded_feed
            .as_ref()
            .map(|loaded| loaded.items.as_slice())
            .unwrap_or(&[])
    }

    pub fn loaded_item_index(&self) -> usize {
        self.loaded_feed
            .as_ref()
            .map(|loaded| loaded.item_index)
            .unwrap_or(0)
    }

    pub fn selected_item(&self) -> Option<&Item> {
        self.loaded_feed
            .as_ref()
            .and_then(|loaded| loaded.selected_item())
    }

    pub fn selected_latest_item(&self) -> Option<&Item> {
        self.latest_items
            .get(self.latest_index)
            .map(|recent| &recent.item)
    }

    pub fn selected_item_for_active_tab(&self) -> Option<&Item> {
        match self.active_tab {
            AppTab::Latest => self.selected_latest_item(),
            AppTab::Reader => self.selected_item(),
        }
    }

    pub fn is_item_read(&self, item_id: &str) -> bool {
        self.item_states
            .get(item_id)
            .map(|s| s.is_read)
            .unwrap_or(false)
    }

    pub fn is_item_starred(&self, item_id: &str) -> bool {
        self.item_states
            .get(item_id)
            .map(|s| s.is_starred)
            .unwrap_or(false)
    }

    pub fn is_item_queued(&self, item_id: &str) -> bool {
        self.item_states
            .get(item_id)
            .map(|s| s.is_queued)
            .unwrap_or(false)
    }

    pub fn is_item_saved(&self, item_id: &str) -> bool {
        self.item_states
            .get(item_id)
            .map(|s| s.is_saved)
            .unwrap_or(false)
    }

    pub fn is_item_archived(&self, item_id: &str) -> bool {
        self.item_states
            .get(item_id)
            .map(|s| s.is_archived)
            .unwrap_or(false)
    }

    pub fn move_up(&mut self) {
        match self.active_pane {
            ActivePane::Feeds => {
                if self.feed_index > 0 {
                    self.feed_index -= 1;
                    self.feed_list_state.select(Some(self.feed_index));
                }
            }
            ActivePane::Items => match self.active_tab {
                AppTab::Latest => {
                    if self.latest_index > 0 {
                        self.latest_index -= 1;
                        self.latest_list_state.select(Some(self.latest_index));
                        self.preview_scroll = 0;
                    }
                }
                AppTab::Reader => {
                    if self
                        .loaded_feed
                        .as_mut()
                        .map(|loaded| loaded.move_up())
                        .unwrap_or(false)
                    {
                        self.preview_scroll = 0;
                    }
                }
            },
            ActivePane::Preview => {
                if self.preview_scroll > 0 {
                    self.preview_scroll = self.preview_scroll.saturating_sub(1);
                }
            }
        }
    }

    pub fn move_down(&mut self) {
        match self.active_pane {
            ActivePane::Feeds => {
                if !self.feeds.is_empty() && self.feed_index < self.feeds.len() - 1 {
                    self.feed_index += 1;
                    self.feed_list_state.select(Some(self.feed_index));
                }
            }
            ActivePane::Items => match self.active_tab {
                AppTab::Latest => {
                    if !self.latest_items.is_empty()
                        && self.latest_index < self.latest_items.len() - 1
                    {
                        self.latest_index += 1;
                        self.latest_list_state.select(Some(self.latest_index));
                        self.preview_scroll = 0;
                    }
                }
                AppTab::Reader => {
                    if self
                        .loaded_feed
                        .as_mut()
                        .map(|loaded| loaded.move_down())
                        .unwrap_or(false)
                    {
                        self.preview_scroll = 0;
                    }
                }
            },
            ActivePane::Preview => {
                self.preview_scroll = self.preview_scroll.saturating_add(1);
            }
        }
    }

    pub fn move_top(&mut self) {
        match self.active_pane {
            ActivePane::Feeds => {
                self.feed_index = 0;
                self.feed_list_state.select(Some(0));
            }
            ActivePane::Items => match self.active_tab {
                AppTab::Latest => {
                    self.latest_index = 0;
                    self.latest_list_state.select(Some(0));
                    self.preview_scroll = 0;
                }
                AppTab::Reader => {
                    if self
                        .loaded_feed
                        .as_mut()
                        .map(|loaded| loaded.move_top())
                        .unwrap_or(false)
                    {
                        self.preview_scroll = 0;
                    }
                }
            },
            ActivePane::Preview => {
                self.preview_scroll = 0;
            }
        }
    }

    pub fn move_bottom(&mut self) {
        match self.active_pane {
            ActivePane::Feeds => {
                self.feed_index = self.feeds.len().saturating_sub(1);
                self.feed_list_state.select(Some(self.feed_index));
            }
            ActivePane::Items => match self.active_tab {
                AppTab::Latest => {
                    self.latest_index = self.latest_items.len().saturating_sub(1);
                    self.latest_list_state.select(Some(self.latest_index));
                    self.preview_scroll = 0;
                }
                AppTab::Reader => {
                    if self
                        .loaded_feed
                        .as_mut()
                        .map(|loaded| loaded.move_bottom())
                        .unwrap_or(false)
                    {
                        self.preview_scroll = 0;
                    }
                }
            },
            ActivePane::Preview => {
                self.preview_scroll = u16::MAX;
            }
        }
    }

    pub fn next_page(&mut self) {
        match self.active_pane {
            ActivePane::Feeds => {
                let max_index = self.feeds.len().saturating_sub(1);
                let new_index = (self.feed_index + PAGE_SIZE).min(max_index);
                if new_index != self.feed_index {
                    self.feed_index = new_index;
                    self.feed_list_state.select(Some(self.feed_index));
                }
            }
            ActivePane::Items => match self.active_tab {
                AppTab::Latest => {
                    let max_index = self.latest_items.len().saturating_sub(1);
                    let new_index = (self.latest_index + PAGE_SIZE).min(max_index);
                    if new_index != self.latest_index {
                        self.latest_index = new_index;
                        self.latest_list_state.select(Some(self.latest_index));
                        self.preview_scroll = 0;
                    }
                }
                AppTab::Reader => {
                    if self
                        .loaded_feed
                        .as_mut()
                        .map(|loaded| loaded.next_page())
                        .unwrap_or(false)
                    {
                        self.preview_scroll = 0;
                    }
                }
            },
            ActivePane::Preview => {
                self.preview_scroll = self.preview_scroll.saturating_add(PAGE_SIZE as u16);
            }
        }
    }

    pub fn prev_page(&mut self) {
        match self.active_pane {
            ActivePane::Feeds => {
                let new_index = self.feed_index.saturating_sub(PAGE_SIZE);
                if new_index != self.feed_index {
                    self.feed_index = new_index;
                    self.feed_list_state.select(Some(self.feed_index));
                }
            }
            ActivePane::Items => match self.active_tab {
                AppTab::Latest => {
                    let new_index = self.latest_index.saturating_sub(PAGE_SIZE);
                    if new_index != self.latest_index {
                        self.latest_index = new_index;
                        self.latest_list_state.select(Some(self.latest_index));
                        self.preview_scroll = 0;
                    }
                }
                AppTab::Reader => {
                    if self
                        .loaded_feed
                        .as_mut()
                        .map(|loaded| loaded.prev_page())
                        .unwrap_or(false)
                    {
                        self.preview_scroll = 0;
                    }
                }
            },
            ActivePane::Preview => {
                self.preview_scroll = self.preview_scroll.saturating_sub(PAGE_SIZE as u16);
            }
        }
    }

    pub fn toggle_maximize(&mut self) {
        self.maximized = !self.maximized;
        if self.maximized {
            self.active_pane = ActivePane::Preview;
        }
    }

    pub fn set_status(&mut self, message: String) {
        self.status_message = Some(message);
    }

    pub fn clear_status(&mut self) {
        self.status_message = None;
    }
}

impl Default for TuiApp {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod loaded_feed_tests {
    use super::*;

    fn make_item(feed_id: i64, idx: usize) -> Item {
        Item::new(
            feed_id,
            &format!("https://example.com/{feed_id}.xml"),
            &format!("entry-{idx}"),
        )
    }

    #[test]
    fn new_with_items_selects_first_row() {
        let loaded = LoadedFeed::new(7, vec![make_item(7, 0), make_item(7, 1)]);
        assert_eq!(loaded.feed_id, 7);
        assert_eq!(loaded.item_index, 0);
        assert_eq!(loaded.item_list_state.selected(), Some(0));
    }

    #[test]
    fn new_empty_clears_list_state() {
        let loaded = LoadedFeed::new(7, vec![]);
        assert_eq!(loaded.item_index, 0);
        assert_eq!(loaded.item_list_state.selected(), None);
    }

    #[test]
    fn cursor_methods_keep_index_and_list_state_in_lockstep() {
        let mut loaded = LoadedFeed::new(7, (0..5).map(|i| make_item(7, i)).collect());
        assert!(loaded.move_down());
        assert_eq!(loaded.item_index, 1);
        assert_eq!(loaded.item_list_state.selected(), Some(1));

        assert!(loaded.move_bottom());
        assert_eq!(loaded.item_index, 4);
        assert_eq!(loaded.item_list_state.selected(), Some(4));

        // saturate at bottom
        assert!(!loaded.move_down());
        assert_eq!(loaded.item_index, 4);

        assert!(loaded.move_top());
        assert_eq!(loaded.item_index, 0);
        assert_eq!(loaded.item_list_state.selected(), Some(0));

        // saturate at top
        assert!(!loaded.move_up());
        assert_eq!(loaded.item_index, 0);
    }

    #[test]
    fn reclamp_after_items_shrink_pulls_cursor_back() {
        let mut loaded = LoadedFeed::new(7, (0..5).map(|i| make_item(7, i)).collect());
        loaded.move_bottom();
        // External truncation (e.g. filter view change re-using a LoadedFeed
        // — though current code rebuilds LoadedFeed instead). reclamp_cursor
        // is the safety net.
        loaded.items.truncate(2);
        loaded.reclamp_cursor();
        assert_eq!(loaded.item_index, 1);
        assert_eq!(loaded.item_list_state.selected(), Some(1));
    }
}

use crossterm::event::{self, Event, KeyEvent};
use std::time::Duration;
use tokio::sync::mpsc;

use crate::app::Result;

#[derive(Debug)]
pub enum AppEvent {
    Key(KeyEvent),
    Tick,
    RefreshProgress(usize, usize),
    RefreshComplete(i64, Vec<(i64, Result<crate::store::FeedRefreshResult>)>),
}

pub struct EventHandler {
    rx: mpsc::UnboundedReceiver<AppEvent>,
    tx: mpsc::UnboundedSender<AppEvent>,
}

impl EventHandler {
    pub fn new(tick_rate: Duration) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let event_tx = tx.clone();

        tokio::spawn(async move {
            loop {
                let has_event =
                    tokio::task::spawn_blocking(move || event::poll(tick_rate).unwrap_or(false))
                        .await
                        .unwrap_or(false);

                if has_event {
                    if let Ok(Event::Key(key)) = event::read() {
                        if event_tx.send(AppEvent::Key(key)).is_err() {
                            break;
                        }
                    }
                }
                if event_tx.send(AppEvent::Tick).is_err() {
                    break;
                }
            }
        });

        Self { rx, tx }
    }

    pub async fn next(&mut self) -> Option<AppEvent> {
        self.rx.recv().await
    }

    pub fn get_tx(&self) -> mpsc::UnboundedSender<AppEvent> {
        self.tx.clone()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Quit,
    MoveUp,
    MoveDown,
    MoveTop,
    MoveBottom,
    NextPage,
    PrevPage,
    NextPane,
    PrevPane,
    FocusLeft,
    FocusRight,
    Select,
    ToggleRead,
    ToggleStar,
    ToggleQueued,
    ToggleSaved,
    ToggleArchived,
    ViewAll,
    ViewUnread,
    ViewStarred,
    ViewQueued,
    ViewSaved,
    ViewArchived,
    ViewLatest,
    ViewReader,
    OpenInBrowser,
    Refresh,
    ToggleMaximize,
    ToggleFeedPanel,
    DeleteFeed,
    WindowChord,
    None,
}

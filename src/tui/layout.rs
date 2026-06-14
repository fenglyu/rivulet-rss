use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Gauge, List, ListItem, Paragraph, Wrap},
    Frame,
};

use crate::config::ColorConfig;
use crate::domain::Item;
use crate::tui::app::{ActivePane, AppTab, FeedPanelState, TuiApp};

pub fn render(frame: &mut Frame, app: &mut TuiApp, colors: &ColorConfig) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(10),
            Constraint::Length(1),
        ])
        .split(frame.area());

    render_tab_strip(frame, app, chunks[0], colors);
    render_body(frame, app, chunks[1], colors);
    render_status_bar(frame, app, chunks[2], colors);
}

fn render_body(frame: &mut Frame, app: &mut TuiApp, area: Rect, colors: &ColorConfig) {
    if app.maximized {
        let selected = app.selected_item_for_active_tab().cloned();
        render_content_pane(frame, selected.as_ref(), app, area, colors, " Content ");
        return;
    }

    match app.active_tab {
        AppTab::Latest => render_latest_tab(frame, app, area, colors),
        AppTab::Reader => render_reader_tab(frame, app, area, colors),
    }
}

fn render_tab_strip(frame: &mut Frame, app: &TuiApp, area: Rect, colors: &ColorConfig) {
    let latest_style = if app.active_tab == AppTab::Latest {
        Style::default()
            .fg(colors.selection_fg_active)
            .bg(colors.selection_bg_active)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(colors.read_item)
    };
    let reader_style = if app.active_tab == AppTab::Reader {
        Style::default()
            .fg(colors.selection_fg_active)
            .bg(colors.selection_bg_active)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(colors.read_item)
    };

    let line = Line::from(vec![
        Span::raw(" "),
        Span::styled("[ Latest ]", latest_style),
        Span::raw("  "),
        Span::styled("[ Reader ]", reader_style),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

fn render_latest_tab(frame: &mut Frame, app: &mut TuiApp, area: Rect, colors: &ColorConfig) {
    if area.width < 80 {
        if app.selected_latest_item().is_some() {
            let selected = app.selected_latest_item().cloned();
            render_content_pane(frame, selected.as_ref(), app, area, colors, " Latest ");
        } else {
            render_latest_items_pane(frame, app, area, colors);
        }
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);

    render_latest_items_pane(frame, app, chunks[0], colors);
    let selected = app.selected_latest_item().cloned();
    render_content_pane(frame, selected.as_ref(), app, chunks[1], colors, " Latest ");
}

fn render_reader_tab(frame: &mut Frame, app: &mut TuiApp, area: Rect, colors: &ColorConfig) {
    if area.width < 80 {
        if app.loaded_feed.is_none() && app.feed_panel == FeedPanelState::Expanded {
            render_feeds_pane(frame, app, area, colors);
        } else if app.loaded_feed.is_none() {
            render_content_pane(frame, None, app, area, colors, " Preview ");
        } else if app.selected_item().is_some() && app.active_pane == ActivePane::Preview {
            let selected = app.selected_item().cloned();
            render_content_pane(frame, selected.as_ref(), app, area, colors, " Preview ");
        } else {
            render_items_pane(frame, app, area, colors);
        }
        return;
    }

    if app.loaded_feed.is_none() {
        let feed_width = match app.feed_panel {
            FeedPanelState::Collapsed => 3,
            FeedPanelState::Expanded => 30,
        };
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(feed_width), Constraint::Percentage(100)])
            .split(area);

        render_feed_rail(frame, app, chunks[0], colors);
        render_content_pane(frame, None, app, chunks[1], colors, " Preview ");
        return;
    }

    let feed_width = match app.feed_panel {
        FeedPanelState::Collapsed => 3,
        FeedPanelState::Expanded => 30,
    };
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(feed_width),
            Constraint::Percentage(35),
            Constraint::Percentage(65),
        ])
        .split(area);

    render_feed_rail(frame, app, chunks[0], colors);
    render_items_pane(frame, app, chunks[1], colors);
    let selected = app.selected_item().cloned();
    render_content_pane(
        frame,
        selected.as_ref(),
        app,
        chunks[2],
        colors,
        " Preview ",
    );
}

fn render_feed_rail(frame: &mut Frame, app: &mut TuiApp, area: Rect, colors: &ColorConfig) {
    match app.feed_panel {
        FeedPanelState::Collapsed => {
            let block = Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(colors.inactive_border));
            frame.render_widget(Paragraph::new("F").block(block), area);
        }
        FeedPanelState::Expanded => render_feeds_pane(frame, app, area, colors),
    }
}

fn render_feeds_pane(frame: &mut Frame, app: &mut TuiApp, area: Rect, colors: &ColorConfig) {
    let is_active = app.active_tab == AppTab::Reader && app.active_pane == ActivePane::Feeds;
    let border_style = if is_active {
        Style::default().fg(colors.active_border)
    } else {
        Style::default().fg(colors.inactive_border)
    };

    let items: Vec<ListItem> = app
        .feeds
        .iter()
        .map(|feed| {
            let unread = app
                .loaded_items()
                .iter()
                .filter(|item| item.feed_id == feed.id && !app.is_item_read(&item.id))
                .count();

            let content = if unread > 0 {
                format!("{} ({})", feed.display_title(), unread)
            } else {
                feed.display_title().to_string()
            };

            ListItem::new(content)
        })
        .collect();

    let title = format!(
        " Feeds ({}) [{}/{}] ",
        app.feeds.len(),
        app.feed_index + 1,
        app.feeds.len().max(1)
    );

    let list = List::new(items)
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(border_style),
        )
        .highlight_style(selection_style(is_active, colors))
        .highlight_symbol("> ");

    frame.render_stateful_widget(list, area, &mut app.feed_list_state);
}

fn render_items_pane(frame: &mut Frame, app: &mut TuiApp, area: Rect, colors: &ColorConfig) {
    let is_active = app.active_tab == AppTab::Reader && app.active_pane == ActivePane::Items;

    // Collect everything we need from `app` under an immutable borrow first
    // so that the `&mut` borrow we take below for `item_list_state` is the
    // only outstanding mutable borrow when we hit `render_stateful_widget`.
    let items: Vec<ListItem<'static>> = app
        .loaded_items()
        .iter()
        .map(|item| render_item_row(app, item, false, colors))
        .collect();
    let title = format!(
        " Items: {} ({}) [{}/{}] ",
        app.item_view.label(),
        app.loaded_items().len(),
        app.loaded_item_index() + 1,
        app.loaded_items().len().max(1)
    );
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(border_style(is_active, colors));
    let highlight = selection_style(is_active, colors);

    let Some(loaded) = app.loaded_feed.as_mut() else {
        // No feed loaded → render the empty-state message with the same block.
        frame.render_widget(
            Paragraph::new("Select a source from the feed list.").block(block),
            area,
        );
        return;
    };

    if loaded.items.is_empty() {
        frame.render_widget(
            Paragraph::new("No items for this source and filter.").block(block),
            area,
        );
        return;
    }

    let list = List::new(items)
        .block(block)
        .highlight_style(highlight)
        .highlight_symbol("> ");

    frame.render_stateful_widget(list, area, &mut loaded.item_list_state);
}

fn render_latest_items_pane(frame: &mut Frame, app: &mut TuiApp, area: Rect, colors: &ColorConfig) {
    let is_active = app.active_tab == AppTab::Latest && app.active_pane == ActivePane::Items;
    let items: Vec<ListItem> = app
        .latest_items
        .iter()
        .map(|recent| render_item_row(app, &recent.item, recent.is_latest_refresh_item, colors))
        .collect();

    let title = format!(
        " Latest: {} ({}) [{}/{}] ",
        app.item_view.label(),
        app.latest_items.len(),
        app.latest_index + 1,
        app.latest_items.len().max(1)
    );

    let empty = if app.latest_items.is_empty() {
        if app.latest_run_id.is_none() {
            "No refresh batch recorded yet - press R to fetch."
        } else {
            "No recent items in the selected window."
        }
    } else {
        ""
    };

    if app.latest_items.is_empty() {
        let paragraph = Paragraph::new(empty)
            .block(
                Block::default()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_style(border_style(is_active, colors)),
            )
            .wrap(Wrap { trim: true });
        frame.render_widget(paragraph, area);
        return;
    }

    let list = List::new(items)
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(border_style(is_active, colors)),
        )
        .highlight_style(selection_style(is_active, colors))
        .highlight_symbol("> ");

    frame.render_stateful_widget(list, area, &mut app.latest_list_state);
}

fn render_item_row(
    app: &TuiApp,
    item: &Item,
    is_latest_refresh_item: bool,
    colors: &ColorConfig,
) -> ListItem<'static> {
    let is_read = app.is_item_read(&item.id);
    let is_starred = app.is_item_starred(&item.id);
    let is_queued = app.is_item_queued(&item.id);
    let is_saved = app.is_item_saved(&item.id);
    let is_archived = app.is_item_archived(&item.id);

    let marker = if is_latest_refresh_item {
        "NEW"
    } else if is_archived {
        "x  "
    } else if is_saved {
        "S  "
    } else if is_queued {
        "Q  "
    } else if is_starred {
        "*  "
    } else if !is_read {
        ".  "
    } else {
        "   "
    };

    let date = item
        .published_at
        .map(|d| d.format("%m/%d").to_string())
        .unwrap_or_else(|| "     ".to_string());
    let content = format!("{} {} {}", marker, date, item.display_title());

    let style = if is_latest_refresh_item {
        Style::default()
            .fg(ratatui::style::Color::LightGreen)
            .add_modifier(Modifier::BOLD)
    } else if !is_read {
        Style::default()
            .fg(colors.unread_item)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(colors.read_item)
    };

    ListItem::new(content).style(style)
}

fn render_content_pane(
    frame: &mut Frame,
    item: Option<&Item>,
    app: &TuiApp,
    area: Rect,
    colors: &ColorConfig,
    fallback_title: &str,
) {
    let is_active = app.active_pane == ActivePane::Preview;
    let (title, content) = if let Some(item) = item {
        let title_text = item.display_title().to_string();
        let mut lines = Vec::new();

        lines.push(Line::from(Span::styled(
            item.display_title(),
            Style::default().add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));

        if let Some(author) = &item.author {
            lines.push(Line::from(Span::styled(
                format!("By: {}", author),
                Style::default().fg(colors.metadata_author),
            )));
        }
        if let Some(date) = item.published_at {
            lines.push(Line::from(Span::styled(
                format!("Date: {}", date.format("%Y-%m-%d %H:%M")),
                Style::default().fg(colors.metadata_date),
            )));
        }
        if let Some(link) = &item.link {
            lines.push(Line::from(Span::styled(
                format!("Link: {}", link),
                Style::default().fg(colors.metadata_link),
            )));
        }
        lines.push(Line::from(""));
        lines.push(Line::from(
            "-".repeat(area.width.saturating_sub(2) as usize),
        ));
        lines.push(Line::from(""));

        let content_text = strip_html(item.display_content());
        for line in content_text.lines() {
            lines.push(Line::from(line.to_string()));
        }

        (format!(" {} ", title_text), Text::from(lines))
    } else {
        (fallback_title.to_string(), Text::from("No item selected"))
    };

    let paragraph = Paragraph::new(content)
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(border_style(is_active, colors)),
        )
        .wrap(Wrap { trim: false })
        .scroll((app.preview_scroll, 0));

    frame.render_widget(paragraph, area);
}

fn render_status_bar(frame: &mut Frame, app: &TuiApp, area: Rect, colors: &ColorConfig) {
    if app.is_refreshing {
        let (current, total) = app.refresh_progress;
        let ratio = if total > 0 {
            (current as f64 / total as f64).min(1.0)
        } else {
            0.0
        };

        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(30), Constraint::Length(30)])
            .split(area);

        let status = format!("Refreshing feeds... ({}/{})", current, total);
        let paragraph = Paragraph::new(status)
            .style(Style::default().fg(colors.status_fg).bg(colors.status_bg));
        frame.render_widget(paragraph, chunks[0]);

        let gauge = Gauge::default()
            .style(
                Style::default()
                    .fg(colors.active_border)
                    .bg(colors.status_bg),
            )
            .ratio(ratio)
            .label(format!("{}%", (ratio * 100.0) as u32))
            .use_unicode(true);
        frame.render_widget(gauge, chunks[1]);
    } else {
        let status = if let Some((_, ref title)) = app.pending_delete {
            format!("Delete \"{}\"? (y/n)", title)
        } else if let Some(ref msg) = app.status_message {
            msg.clone()
        } else if app.maximized {
            "j/k:Scroll  g/G/%:Top/Bottom  n/p:Page  m:Exit maximize  [/]:Tabs  q:Quit".to_string()
        } else {
            "[/]:Tabs  \\:Feeds  j/k/g/G/%:Nav  a/u/f/l/v/X:Views  r/s/L/S/x/o:Actions  R:Refresh  q:Quit"
                .to_string()
        };

        let paragraph = Paragraph::new(status)
            .style(Style::default().fg(colors.status_fg).bg(colors.status_bg));

        frame.render_widget(paragraph, area);
    }
}

fn border_style(is_active: bool, colors: &ColorConfig) -> Style {
    if is_active {
        Style::default().fg(colors.active_border)
    } else {
        Style::default().fg(colors.inactive_border)
    }
}

fn selection_style(is_active: bool, colors: &ColorConfig) -> Style {
    if is_active {
        Style::default()
            .bg(colors.selection_bg_active)
            .fg(colors.selection_fg_active)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .bg(colors.selection_bg_inactive)
            .fg(colors.selection_fg_inactive)
    }
}

fn strip_html(html: &str) -> String {
    let mut result = String::new();
    let mut in_tag = false;
    let mut last_was_space = false;

    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => {
                if c.is_whitespace() {
                    if !last_was_space {
                        result.push(' ');
                        last_was_space = true;
                    }
                } else {
                    result.push(c);
                    last_was_space = false;
                }
            }
            _ => {}
        }
    }

    result.trim().to_string()
}

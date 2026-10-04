use gpui::{
    App, Application, Bounds, Context, Entity, KeyBinding, Menu, MenuItem, SharedString,
    Subscription, Window, WindowBounds, WindowOptions, actions, div, prelude::*, px, rgb, size,
};
use gpui_component::{
    Disableable, Root, Selectable, Sizable, Theme, ThemeMode,
    button::{Button, ButtonVariants},
    input::{Input, InputEvent, InputState},
    text::TextView,
};
use rivulet::store::ItemListFilter;
use rivulet_desktop::{
    service::{Article, Command, Event, Filter, Snapshot, Worker, default_data_dir},
    sources::{Access, Publisher},
};

actions!(rivulet_desktop, [Quit]);

const INK: u32 = 0x19292c;
const MUTED: u32 = 0x6f7b7d;
const PAPER: u32 = 0xfafaf7;
const LINE: u32 = 0xe2e6e2;
const ACCENT: u32 = 0x246c5b;

struct NewsDesk {
    worker: Worker,
    snapshot: Snapshot,
    selected: Option<String>,
    filter: Filter,
    search: Entity<InputState>,
    link: Entity<InputState>,
    status: String,
    busy: bool,
    _subscriptions: Vec<Subscription>,
}

impl NewsDesk {
    fn new(worker: Worker, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("搜索标题与已保存正文"));
        let link = cx.new(|cx| {
            InputState::new(window, cx).placeholder("粘贴 Bloomberg、财新或南华早报文章链接")
        });
        let subscription = cx.subscribe(&search, |this, input, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.filter.query = input.read(cx).value().to_string();
                this.send(Command::Load(this.filter.clone()), cx);
            }
        });
        let shutdown_commands = worker.commands.clone();
        let shutdown = cx.on_app_quit(move |_, _| {
            let (done, receiver) = async_channel::bounded(1);
            let _ = shutdown_commands.try_send(Command::Shutdown(done));
            async move {
                let _ = receiver.recv().await;
            }
        });
        let receiver = worker.events.clone();
        cx.spawn(async move |view, cx| {
            while let Ok(event) = receiver.recv().await {
                if view
                    .update(cx, |this, cx| {
                        match event {
                            Event::Busy(message) => {
                                this.busy = true;
                                this.status = message;
                            }
                            Event::Updated(snapshot, message) => {
                                this.busy = false;
                                if !snapshot
                                    .articles
                                    .iter()
                                    .any(|article| Some(&article.item.id) == this.selected.as_ref())
                                {
                                    this.selected = snapshot
                                        .articles
                                        .first()
                                        .map(|article| article.item.id.clone());
                                }
                                this.snapshot = snapshot;
                                this.status = message;
                            }
                            Event::Failed(message) => {
                                this.busy = false;
                                this.status = message;
                            }
                        }
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        let _ = worker.commands.try_send(Command::Load(Filter::default()));
        Self {
            worker,
            snapshot: Snapshot::default(),
            selected: None,
            filter: Filter::default(),
            search,
            link,
            status: "正在载入本地资料…".into(),
            busy: false,
            _subscriptions: vec![subscription, shutdown],
        }
    }

    fn send(&mut self, command: Command, cx: &mut Context<Self>) {
        if self.worker.commands.try_send(command).is_err() {
            self.status = "后台繁忙或已停止，请稍后重试".into();
        } else {
            self.busy = true;
        }
        cx.notify();
    }

    fn select_source(&mut self, publisher: Option<Publisher>, cx: &mut Context<Self>) {
        self.filter.publisher = publisher;
        self.send(Command::Load(self.filter.clone()), cx);
    }

    fn selected_article(&self) -> Option<&Article> {
        self.snapshot
            .articles
            .iter()
            .find(|article| Some(&article.item.id) == self.selected.as_ref())
    }

    fn import_emails(&mut self, cx: &mut Context<Self>) {
        let picker = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("选择导出的订阅邮件（.eml）".into()),
        });
        cx.spawn(async move |view, cx| match picker.await {
            Ok(Ok(Some(paths))) => {
                let _ = view.update(cx, |this, cx| this.send(Command::ImportEmails(paths), cx));
            }
            Ok(Ok(None)) => {}
            _ => {
                let _ = view.update(cx, |this, cx| {
                    this.status = "无法打开文件选择器".into();
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(225.))
            .h_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap_4()
            .p_5()
            .bg(rgb(0x182d30))
            .text_color(rgb(0xe7eeeb))
            .child(
                div()
                    .text_2xl()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child("Rivulet"),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(0x9eb9b0))
                    .child("你的订阅 · 一处阅读"),
            )
            .child(
                div()
                    .mt_4()
                    .text_xs()
                    .text_color(rgb(0x9eb9b0))
                    .child("阅读"),
            )
            .child(
                Button::new("all-sources")
                    .ghost()
                    .label("全部新闻")
                    .selected(self.filter.publisher.is_none())
                    .text_color(rgb(if self.filter.publisher.is_none() {
                        INK
                    } else {
                        0xe7eeeb
                    }))
                    .on_click(cx.listener(|this, _, _, cx| this.select_source(None, cx))),
            )
            .children(
                [
                    ("all", "全部", ItemListFilter::All),
                    ("unread", "未读", ItemListFilter::Unread),
                    ("saved", "收藏", ItemListFilter::Saved),
                ]
                .into_iter()
                .map(|(id, label, view)| {
                    Button::new(id)
                        .ghost()
                        .label(label)
                        .selected(self.filter.view == view)
                        .text_color(rgb(if self.filter.view == view {
                            INK
                        } else {
                            0xe7eeeb
                        }))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.filter.view = view;
                            this.send(Command::Load(this.filter.clone()), cx);
                        }))
                }),
            )
            .child(
                div()
                    .mt_3()
                    .text_xs()
                    .text_color(rgb(0x9eb9b0))
                    .child("新闻源"),
            )
            .children(Publisher::ALL.into_iter().map(|publisher| {
                let count = self
                    .snapshot
                    .sources
                    .iter()
                    .find(|source| source.publisher == publisher)
                    .map(|source| source.count)
                    .unwrap_or(0);
                div().flex().flex_col().gap_1().child(
                    Button::new(SharedString::from(format!("source-{}", publisher.key())))
                        .ghost()
                        .label(format!("{}   {}", publisher.name(), count))
                        .selected(self.filter.publisher == Some(publisher))
                        .text_color(rgb(if self.filter.publisher == Some(publisher) {
                            INK
                        } else {
                            0xe7eeeb
                        }))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.select_source(Some(publisher), cx)
                        })),
                )
            }))
            .child(div().flex_1())
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(0x9eb9b0))
                    .child("内容与登录会话保存在本机"),
            )
    }

    fn connections(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .gap_3()
            .children(Publisher::ALL.into_iter().map(|publisher| {
                let status = self
                    .snapshot
                    .sources
                    .iter()
                    .find(|source| source.publisher == publisher)
                    .map(|source| source.status.clone())
                    .unwrap_or_else(|| "尚未连接".into());
                let auto_sync = self
                    .snapshot
                    .sources
                    .iter()
                    .find(|source| source.publisher == publisher)
                    .is_some_and(|source| source.auto_sync);
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_3()
                    .rounded_lg()
                    .bg(rgb(0xffffff))
                    .border_1()
                    .border_color(rgb(LINE))
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(publisher.name()),
                    )
                    .child(div().text_xs().text_color(rgb(MUTED)).child(status))
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                Button::new(SharedString::from(format!(
                                    "login-{}",
                                    publisher.key()
                                )))
                                .small()
                                .label("登录账号")
                                .disabled(self.busy)
                                .on_click(cx.listener(
                                    move |this, _, _, cx| this.send(Command::Login(publisher), cx),
                                )),
                            )
                            .child(
                                Button::new(SharedString::from(format!(
                                    "sync-{}",
                                    publisher.key()
                                )))
                                .small()
                                .label("同步标题")
                                .disabled(self.busy)
                                .on_click(cx.listener(
                                    move |this, _, _, cx| this.send(Command::Sync(publisher), cx),
                                )),
                            ),
                    )
                    .child(
                        Button::new(SharedString::from(format!("auto-{}", publisher.key())))
                            .small()
                            .ghost()
                            .label(if auto_sync {
                                "自动同步 · 每 15 分钟"
                            } else {
                                "启用自动同步"
                            })
                            .selected(auto_sync)
                            .disabled(self.busy)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.send(Command::SetAutoSync(publisher, !auto_sync), cx)
                            })),
                    )
            }))
    }

    fn article_list(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut list = div()
            .id("article-list")
            .w(px(330.))
            .h_full()
            .flex_shrink_0()
            .overflow_y_scroll()
            .border_r_1()
            .border_color(rgb(LINE));
        if self.snapshot.articles.is_empty() {
            return list.child(div().p_6().flex().flex_col().gap_3()
                .child(div().text_lg().child("等待你的新闻"))
                .child(div().text_sm().text_color(rgb(MUTED))
                    .child("登录新闻源后同步标题，或导入订阅邮件。你也可以直接粘贴一篇付费文章的链接。")));
        }
        for article in &self.snapshot.articles {
            let id = article.item.id.clone();
            let selected = self.selected.as_ref() == Some(&id);
            let date = match article.item.published_at {
                Some(date) => format!("发布于 {}", date.format("%m-%d %H:%M UTC")),
                None => format!(
                    "收录于 {}",
                    article.item.fetched_at.format("%m-%d %H:%M UTC")
                ),
            };
            list = list.child(
                div()
                    .id(SharedString::from(id.clone()))
                    .cursor_pointer()
                    .p_4()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .border_b_1()
                    .border_color(rgb(LINE))
                    .bg(rgb(if selected { 0xeaf2ee } else { PAPER }))
                    .hover(|style| style.bg(rgb(0xf0f4ef)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.selected = Some(id.clone());
                        cx.notify();
                    }))
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .text_xs()
                            .text_color(rgb(ACCENT))
                            .child(article.publisher.name())
                            .child(if article.is_read { "已读" } else { "未读" }),
                    )
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .child(article.item.display_title().to_owned()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(MUTED))
                            .child(date.to_string()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(MUTED))
                            .child(article.access.label()),
                    ),
            );
        }
        list
    }

    fn reader(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(article) = self.selected_article() else {
            return div()
                .flex_1()
                .h_full()
                .p_8()
                .flex()
                .flex_col()
                .gap_4()
                .child(div().text_3xl().child("把值得读的新闻放在一起"))
                .child(
                    div()
                        .text_color(rgb(MUTED))
                        .child("Bloomberg · 财新 · 南华早报"),
                )
                .child(
                    div()
                        .mt_4()
                        .text_sm()
                        .text_color(rgb(MUTED))
                        .child("第一步：点击上方“登录账号”，在原站正常登录，然后同步标题。"),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(MUTED))
                        .child("收到的订阅邮件可从邮件客户端导出为 .eml 后导入。"),
                )
                .into_any_element();
        };
        let id = article.item.id.clone();
        let save_id = id.clone();
        let is_saved = article.is_saved;
        let is_read = article.is_read;
        let url = article.item.link.clone();
        let capture_url = url.clone();
        let open_url = url.clone();
        let body = article
            .item
            .content
            .as_deref()
            .filter(|text| !text.is_empty())
            .or(article.item.summary.as_deref())
            .unwrap_or("这篇文章尚未读取正文。点击“读取正文”，或在原站阅读。");
        // Render escaped local text as native paragraphs; remote tracking assets never load.
        let escaped = body
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");
        let html = format!(
            "<p>{}</p>",
            escaped.replace("\n\n", "</p><p>").replace('\n', "<br>")
        );
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .px_6()
                    .py_4()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .border_b_1()
                    .border_color(rgb(LINE))
                    .child(div().text_xs().text_color(rgb(ACCENT)).child(format!(
                        "{}  /  {}",
                        article.publisher.name(),
                        article.access.label()
                    )))
                    .child(
                        div()
                            .text_2xl()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(article.item.display_title().to_owned()),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                Button::new("read-body")
                                    .small()
                                    .primary()
                                    .label("读取正文")
                                    .disabled(
                                        self.busy
                                            || capture_url.is_none()
                                            || article.access == Access::Newsletter,
                                    )
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if let Some(url) = &capture_url {
                                            this.send(Command::Capture(url.clone()), cx);
                                        }
                                    })),
                            )
                            .child(
                                Button::new("open-original")
                                    .small()
                                    .label("原站打开")
                                    .disabled(self.busy || open_url.is_none())
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if let Some(url) = &open_url {
                                            this.send(Command::OpenArticle(url.clone()), cx);
                                        }
                                    })),
                            )
                            .child(
                                Button::new("mark-read")
                                    .small()
                                    .label(if is_read {
                                        "标为未读"
                                    } else {
                                        "标为已读"
                                    })
                                    .disabled(self.busy)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.send(Command::SetRead(id.clone(), !is_read), cx)
                                    })),
                            )
                            .child(
                                Button::new("save-item")
                                    .small()
                                    .label(if is_saved { "取消收藏" } else { "收藏" })
                                    .disabled(self.busy)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.send(Command::SetSaved(save_id.clone(), !is_saved), cx)
                                    })),
                            ),
                    ),
            )
            .child(
                div()
                    .id("reader-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_6()
                    .child(TextView::html("article-content", html, window, cx).selectable(true)),
            )
            .into_any_element()
    }
}

impl Render for NewsDesk {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let sidebar = self.sidebar(cx).into_any_element();
        let connections = self.connections(cx).into_any_element();
        let articles = self.article_list(cx).into_any_element();
        let reader = self.reader(window, cx).into_any_element();
        let title = self
            .filter
            .publisher
            .map(Publisher::name)
            .unwrap_or("全部新闻");
        div()
            .size_full()
            .flex()
            .bg(rgb(PAPER))
            .text_color(rgb(INK))
            .font_family(".AppleSystemUIFont")
            .child(sidebar)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .p_5()
                            .flex()
                            .flex_col()
                            .gap_4()
                            .border_b_1()
                            .border_color(rgb(LINE))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_4()
                                    .child(
                                        div()
                                            .flex_1()
                                            .text_2xl()
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .child(title),
                                    )
                                    .child(
                                        div().w(px(270.)).child(Input::new(&self.search).small()),
                                    )
                                    .child(
                                        Button::new("import-emails")
                                            .label("导入邮件")
                                            .disabled(self.busy)
                                            .on_click(
                                                cx.listener(|this, _, _, cx| {
                                                    this.import_emails(cx)
                                                }),
                                            ),
                                    ),
                            )
                            .child(connections)
                            .child(
                                div()
                                    .flex()
                                    .gap_2()
                                    .items_center()
                                    .child(div().flex_1().child(Input::new(&self.link).small()))
                                    .child(
                                        Button::new("add-link")
                                            .small()
                                            .primary()
                                            .label("添加并读取")
                                            .disabled(self.busy)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                let url = this.link.read(cx).value().to_string();
                                                this.send(Command::Capture(url), cx);
                                            })),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .flex()
                            .child(articles)
                            .child(reader),
                    )
                    .child(
                        div()
                            .px_5()
                            .py_2()
                            .border_t_1()
                            .border_color(rgb(LINE))
                            .text_xs()
                            .text_color(rgb(if self.busy { ACCENT } else { MUTED }))
                            .child(self.status.clone()),
                    ),
            )
    }
}

fn main() -> anyhow::Result<()> {
    let data_dir = std::env::var_os("RIVULET_DESKTOP_DATA_DIR")
        .map(std::path::PathBuf::from)
        .map(Ok)
        .unwrap_or_else(default_data_dir)?;
    let worker = Worker::start(data_dir);
    Application::new()
        .with_assets(gpui_component_assets::Assets)
        .run(move |cx: &mut App| {
            gpui_component::init(cx);
            Theme::change(ThemeMode::Light, None, cx);
            cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
            cx.on_action(|_: &Quit, cx| cx.quit());
            cx.set_menus(vec![Menu {
                name: "Rivulet".into(),
                items: vec![MenuItem::action("退出 Rivulet", Quit)],
            }]);
            let bounds = Bounds::centered(None, size(px(1320.), px(900.)), cx);
            let result = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    window_min_size: Some(size(px(1100.), px(700.))),
                    app_id: Some("com.rivulet.desktop".into()),
                    ..WindowOptions::default()
                },
                |window, cx| {
                    window.set_window_title("Rivulet — 订阅新闻");
                    let view = cx.new(|cx| NewsDesk::new(worker, window, cx));
                    cx.new(|cx| Root::new(view, window, cx))
                },
            );
            if let Err(error) = result {
                eprintln!("无法打开桌面窗口：{error:#}");
                cx.quit();
            }
            cx.activate(true);
        });
    Ok(())
}

# Rivulet Desktop

Rivulet 的 macOS GPUI 桌面原型：把 Bloomberg.com、财新和南华早报的网站新闻放入一个本地收件箱。

## 运行

技术选型与性能判断见 [GPUI 桌面技术选型总结](../docs/GPUI_DESKTOP_ASSESSMENT.md)。

```sh
cargo run --manifest-path desktop/Cargo.toml --locked
```

需要 Rust 1.88+、Xcode Command Line Tools，以及 Chrome 或 Chromium。官方 GPUI 当前发布版本为 `0.2.2`，精确锁定该版本；组件库锁定与它兼容的 `0.5.1`。GPUI 使用运行时 Metal 着色器编译，因此无需额外下载 Xcode 的 Metal Toolchain。

生成可双击的 macOS 应用：

```sh
desktop/scripts/build-macos.sh debug
open desktop/dist/Rivulet.app
```

省略 `debug` 则构建 release 版本。脚本使用本地 ad hoc 签名；当前包尚未使用 Developer ID 签名、公证或发布。

## 网站订阅使用流程

1. 点击新闻源的“登录账号”。应用打开该新闻源专属的可见 Chrome 窗口，你在原站输入密码、完成双重验证。
2. 登录后点击“同步标题”。应用关闭自己的可见浏览器，等待配置目录解锁，再以同一配置目录启动 headless Chrome，读取首页当前展示的新闻链接。
3. 选择新闻，点击“读取正文”。只提取当前页面可见的文章段落。标题、摘要、网页正文和需要登录的状态分开保存。
4. 遇到登录过期、人机验证、付费提示或无法识别的正文，点击“原站打开”处理后重试。应用会保留已有正文。
5. 首次成功同步后，该新闻源每 15 分钟刷新标题，应用须保持运行。可在连接卡片暂停；同步失败会自动暂停。

三个来源使用独立 Chrome 配置目录，并在浏览器重启时恢复该目录的登录会话。应用不保存密码、不导出 Cookie，也不复用你正在使用的个人 Chrome 配置目录。需要重新在这些专属会话中登录。

统一列表支持新闻源、未读、收藏和本地全文搜索，支持中文片段和带标点的名称；用空格分隔的多个词需同时匹配。未知发布时间标为“收录时间”，不会假装成最新发布时间。列表最多显示 500 条。

## 接入范围和待验证项

这是可运行的接入原型，尚未用真实订阅账号验证三个网站的付费全文。网站首页链接和正文选择器可能随网站改版变化；反机器人验证可能阻止 headless 读取。检测到可见的订阅墙时不保存它作为正文，检测本身也可能不覆盖网站的所有变体。

同步覆盖首页中可识别的文章链接，不保证覆盖网站全部栏目、全部新闻或手机推送。不支持 Bloomberg Terminal。当前未调用任何订阅私有 API，未实现基于请求头的同步。

邮件客户端导出的 Bloomberg、财新、南华早报 `.eml` 订阅邮件也可导入；这只是补充入口，尚未接入邮箱自动同步。邮件以本地文本显示，不加载远程图片或追踪像素。

桌面版独立数据目录（macOS）：`~/Library/Application Support/rivulet-desktop/`。数据包含订阅正文及浏览器登录会话，保留在本机。原 CLI/TUI 数据目录不变。测试时可通过 `RIVULET_DESKTOP_DATA_DIR` 指定隔离目录。

## 验证

```sh
cargo fmt --manifest-path desktop/Cargo.toml -- --check
cargo clippy --manifest-path desktop/Cargo.toml --all-targets --locked -- -D warnings
cargo test --manifest-path desktop/Cargo.toml --no-default-features --locked
cargo test --manifest-path desktop/Cargo.toml --no-default-features --test browser_smoke --locked -- --ignored
```

浏览器测试使用本地 HTTP 测试页面及临时 Chrome 配置目录，验证 headless 页面读取、隐藏段落排除、付费墙识别，以及重启后保留会话 Cookie 和 localStorage。它不访问用户的订阅账号。

官方接入参考：[GPUI](https://gpui.rs/)、[Bloomberg 订阅者邮件](https://www.bloomberg.com/help/question/newsletters/)、[南华早报 RSS](https://www.scmp.com/rss)、[财新电邮订阅](https://corp.caixin.com/helpemailsub/)。RSS 和邮件入口不代表付费全文 API。

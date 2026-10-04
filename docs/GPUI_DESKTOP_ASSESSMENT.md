# GPUI 桌面技术选型总结

记录日期：2026-10-04。适用项目：Rivulet Desktop。

## 判断

认同 GPUI 对高频交互桌面工具的潜力，但“轻松跑满 120 FPS、极低 CPU 和内存”以及“全面替代 Electron/Tauri”的结论说得过满。趋势观察可以启发选型，性能结论需要实际测量。

## Zeron 值得借鉴的部分

Zeron 的架构文档说明，其流畅体验依赖大量具体优化：列表虚拟化、行高缓存、Markdown 增量解析、后台语法高亮，以及避免动画触发布局。这些都需要开发者实现，换成 GPUI 并不会自动获得。

参考：[Zeron 架构文档](https://github.com/zeronsh/zeron/blob/main/ARCHITECTURE.md)。

## 对相关说法的修正

### 性能数字需要测量依据

验证 120 FPS、CPU 和内存占用，需要说明硬件、文本长度、并发输出量，并比较帧时间、输入延迟和全部进程的内存。演示视频无法单独证明这些结论。

当前 Rivulet Desktop 尚未做这种性能基准测试，不能以窗口观感宣称性能领先。

### Web 渲染也使用 GPU

Chromium 同样使用 GPU 绘制和合成，部分滚动与动画可以跳过布局、绘制阶段。真正要检查的是更新是否触发大量重排、是否阻塞主线程，以及列表是否虚拟化。GPUI 提供了更直接的控制，但仍然需要处理文本解析与排版。

参考：[Chromium 渲染架构](https://developer.chrome.com/docs/chromium/renderingng-architecture)。

### Rust 后台并发并非 GPUI 独有

Tauri 的 Rust 后端也能承担并发调度。Tauri 选择系统 WebView，换取 Web 组件复用和较小的分发体积，同时承担不同平台 WebView 行为差异。这是一项工程取舍，称为“妥协”并没有解释它是否适合具体产品。

参考：[Tauri 进程模型](https://tauri.app/concept/process-model/)。

### 跨平台支持需要区分版本与分支

Zed 已在 2025 年 10 月正式发布 Windows 版；GPUI CE 则明确表示，它与上游的 API 兼容关系正在变化。是否能稳定支持一个具体应用，仍需按实际依赖版本验证。

参考：[Zed Windows 发布说明](https://zed.dev/blog/zed-for-windows-is-here)、[GPUI CE](https://github.com/gpui-ce/gpui-ce)。

## 对 Rivulet Desktop 的选择

继续使用 GPUI：项目已有 Rust 后端，界面主要是新闻列表、搜索和阅读区，这个选择与产品形态匹配。目前运行与打包验证针对 macOS，不代表已完成 Windows 或 Linux 移植。

订阅同步仍然会运行 Chrome。即使界面使用 GPUI，整套应用也会产生浏览器进程的资源开销，不能直接宣称“极低内存”。性能测量需要覆盖 UI 与浏览器进程。

后续工作的优先级：

1. 验证 Bloomberg.com、财新和南华早报真实订阅账号的登录保持、同步覆盖和正文提取可靠性。
2. 优化长文与长列表的阅读体验。
3. 用实际帧时间、输入延迟和全部进程的资源占用评估性能，避免仅依赖观感。

GPUI 能改善阅读信息的体验；能否持续拿到用户关心的信息，更直接决定项目价值。三个来源的真实付费账号与正文读取仍待逐站验证。

桌面版的实现范围、依赖版本及运行方法见 [桌面版使用说明](../desktop/README.md)。

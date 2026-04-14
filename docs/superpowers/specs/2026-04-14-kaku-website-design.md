# Kaku 官方网站设计方案

**日期**：2026-04-14
**作者**：brainstorm session
**状态**：Draft · 待实施
**目标**：为 Kaku 终端设计官方网站，包含落地页与文档站

---

## 1. 目标与范围

### 1.1 产品背景

Kaku 是基于 WezTerm 深度定制的终端，面向 AI 编码场景。核心差异化：

- 零配置开箱即用（JetBrains Mono 默认、macOS 字体渲染、明暗自动切换）
- 内建 AI 助手（错误自动修复、自然语言转命令、AI Tools 面板集成）
- 完全兼容 WezTerm 的 Lua 配置
- 二进制比上游小 40%
- 集成 Lazygit / Yazi / Zsh 插件

### 1.2 网站目标

按重要性排序：

1. **转化下载** —— 落地页在 3 秒内让访客决定是否下载
2. **承载文档** —— 把现有 `docs/*.md` 做成可搜索、可导航的文档站
3. **建立信任** —— 展示作者背景（Tw93 · Mole · Pake）、开源许可、路线图
4. **抢占心智** —— 给 iTerm2 / WezTerm 用户一条清晰的迁移叙事

### 1.3 成功指标（非强制）

- 首屏 3 秒内可点击下载按钮
- `/docs` 可被 Pagefind 本地全文搜索覆盖
- Lighthouse 性能 / 可访问性 / 最佳实践均 ≥ 95
- 中英双语切换无需刷新

---

## 2. 核心决策（已拍板）

| 维度 | 决策 |
|---|---|
| 网站类型 | 落地页 + 文档站（非纯单页、非完整产品站） |
| 主语言 | 中文为主，英文可切换 |
| 技术栈 | Astro + Starlight |
| 部署平台 | GitHub Pages |
| 视觉调性 | Terminal Hacker（深黑底 + 荧光绿 + JetBrains Mono） |
| 搜索 | Starlight 内置 Pagefind（本地静态搜索，不依赖外部服务） |
| 分析统计 | 不上（不接 GA / Plausible） |

---

## 3. 信息架构

### 3.1 顶层路由

```
kaku.site/
├── /                  落地页
├── /docs              文档首页（任务导向入口）
│   ├── /docs/start          快速开始
│   ├── /docs/ai             AI 功能
│   ├── /docs/config         配置
│   ├── /docs/features       功能详解
│   └── /docs/reference      参考
├── /download          下载页
├── /changelog         更新日志（从 GitHub Releases 同步）
└── /roadmap           路线图（Now / Next / Later 三段式）
```

**首发不交付**：`/blog`、`/showcase`。`/blog` 完全不做；`/showcase` 首版不做，v2 迭代。

### 3.2 /docs 子结构

完整的文档分类（对应现有 `docs/*.md` 及其拆分）：

```
快速开始
├── 安装
├── 首次配置
└── 从 iTerm2 / WezTerm 迁移

AI 功能
├── 错误自动修复
├── 自然语言转命令
├── Provider 配置（OpenAI / Claude / ...）
└── AI Tools 面板（Claude Code / Codex / Gemini CLI ...）

配置
├── Lua 配置（兼容 WezTerm）
├── 主题与配色
├── 字体与渲染
└── 快捷键

功能详解
├── 标签与窗口
├── 分屏与广播输入
├── Lazygit / Yazi 集成
└── Shell 集成

参考
├── CLI 命令
├── FAQ
└── 更新日志
```

**首版实现策略**：先把现有 5 个 md 文件（cli.md / configuration.md / faq.md / features.md / keybindings.md）直接导入 Starlight，按上述分类归位，缺失的子条目用 stub 占位页。

### 3.3 全局导航

**顶部**：`Kaku · 文档 · 下载 · Roadmap · Changelog · GitHub ★` + 语言切换（中 / EN） + GitHub star 数 badge

**页脚**（4 列）：

| 产品 | 文档 | 社区 | 项目 |
|---|---|---|---|
| 下载 | 快速开始 | GitHub | Roadmap |
| Changelog | AI 功能 | Issues | License |
| 主题 | 配置 | X/Twitter | 捐赠 |
| | FAQ | | 作者 |

底部版权条：`Kaku · Built on WezTerm · MIT License · © 2026 Tw93`

`Built on WezTerm` 在显眼位置是礼貌和许可合规要求。

---

## 4. 首页设计（9 个 Section）

整页是深黑背景的可滚动"大终端"，每段以 CLI 提示符风格的 `[N] SECTION_TAG` 作为分隔。

### Section 1 · Hero + 下载 CTA
- CLI 提示符装饰：`~/kaku $ launch`
- 主标题：`A fast terminal for AI coding.`
- 副标题：`为 AI 编码而生的终端 · WezTerm 深度定制 · 零配置开箱即用`
- 两个 CTA：`↓ Download DMG` · `brew install tw93/tap/kaku`（点击复制）
- 指标条：GitHub stars · 二进制大小对比 · 平台支持

### Section 2 · Live Terminal Demo
- **实现方式：CSS + JS 打字机动画**（不用 GIF、不用 asciinema）
- 演示场景：错误输入 → Kaku 提示修复 → 用户接受 → 修复执行成功
- 例子：
  ```
  ~/proj $ npm run buidl
  npm ERR! Missing script: "buidl"
  💡 Kaku: Did you mean `npm run build`? Press ⌘⇧E to apply
  ~/proj $ npm run build ✓
  ```
- 动画循环播放，光标闪烁，自然停顿

### Section 3 · 6 核心特性网格
3×2 网格，每格一图标 + 标题 + 一句描述，点击跳对应文档页：

| 特性 | 描述 |
|---|---|
| 零配置 | JetBrains Mono 默认 |
| AI 内建 | 错误修复 / NL 转命令 |
| 主题感知 | 跟随系统明暗 |
| GPU 渲染 | 继承 WezTerm |
| Lua 兼容 | WezTerm 配置零迁移 |
| 集成全家桶 | Lazygit / Yazi / Zsh |

### Section 4 · AI 功能深度展示
- 聚焦"自然语言 → 命令"这个第二差异化卖点
- 展示：输入 `# 把今天的修改推到 feature 分支` → 自动生成 `git add . && git commit -m "..." && git push -u origin feature`
- 旁边一行小字说明：`⌘⇧A 打开 AI 面板，# 前缀触发自然语言，⌘⇧E 应用修复建议`

### Section 5 · 从 iTerm2 / WezTerm 迁移
- 双标签切换：`我是 iTerm2 用户` / `我是 WezTerm 用户`
- iTerm2 标签内容：保留 Cmd+T 习惯 · 启动更快 · 送你一套 AI
- WezTerm 标签内容：lua 配置零迁移 · 体积更小 · 默认更好看
- 每个标签底部有"完整迁移指南 →"链接到 `/docs/start/migrate-from-iterm2` 或 `/docs/start/migrate-from-wezterm`

### Section 6 · 主题与截图廊
- 网格 2 行 × 3 列，共 6 张官方截图
- 深浅主题各 3 张
- 点击放大（lightbox）
- v2 开放社区投稿到 `/showcase`

### Section 7 · 快速开始（代码块）
- 三步走：`brew install tw93/tap/kaku` → `kaku` → 开始使用
- 或者"下载 DMG" 链接到 `/download`
- 代码块一键复制

### Section 8 · Why Kaku · 作者自述
- 一段自述文字 + 作者头像 + Twitter 链接
- 提及 Mole / Pake 背景，建立"这个人做过靠谱的东西"的信任感
- 约 80-120 字

### Section 9 · FAQ + 最终 CTA
- 6 个高频问题折叠列表：和 iTerm2 区别？和 Warp 区别？收费吗？开源吗？Windows 支持？数据隐私？
- 底部再放一次下载按钮（与 Hero 一致）

---

## 5. 文档区设计

### 5.1 /docs 落地页

不是裸目录树，而是任务导向入口卡：

```
┌─────────────────────────────────────┐
│  Kaku 文档                          │
│  为 AI 编码而生的终端               │
├─────────────────────────────────────┤
│  🚀 我是新用户    → 5 分钟快速开始   │
│  🔧 我要配置      → Lua 配置指南     │
│  🤖 我想用 AI     → AI 功能总览      │
│  ⌨️  我要查快捷键  → 快捷键速查       │
│  🔄 我从 X 迁移   → iTerm2 / WezTerm │
└─────────────────────────────────────┘
```

下方附"最近更新"列表，从 git log 拉 5 条最新改动的 md 文件。

### 5.2 单篇文档页组件

| 组件 | 说明 |
|---|---|
| 面包屑 | Starlight 默认 |
| 右侧锚点 | Starlight 默认 |
| 上/下一篇 | Starlight 默认 |
| 代码块复制按钮 | Starlight 默认 |
| `<kbd>` 键位卡片 | 展示 `⌘⇧A` 等快捷键时用自定义组件，不要纯文本 |
| Callout（tip/warning/note） | 三种样式 + 图标 |
| "在 GitHub 编辑本页"底部链接 | 鼓励社区贡献 |
| "WezTerm 同样适用"标记 | 兼容配置页使用 |

### 5.3 搜索

Starlight 内置 Pagefind，本地静态索引，无需外部服务。中英双语各一个独立索引。

---

## 6. /roadmap 实现

最简实现：单个 md 页面，三段式列表：

```
Now     —— 本迭代正在做的 3-5 项
Next    —— 下迭代计划的 5-10 项
Later   —— 有想法但未排期的长列表
```

每项带 GitHub Issue 链接（可选）。不做看板、不接 API。v2 再考虑接 GitHub Projects。

---

## 7. 视觉规范

### 7.1 配色

| 用途 | 色值 |
|---|---|
| 背景主色 | `#0a0a0a` |
| 背景副色 | `#050505` |
| 分隔线 | `#1f1f1f`（实线）/ `dashed` |
| 正文文字 | `#e5e5e5` |
| 次要文字 | `#888` |
| 强调色（primary accent） | `#00ff9f`（荧光绿） |
| 错误色 | `#ff5c5c` |
| 代码块背景 | `#000` |

### 7.2 字体

| 用途 | 字体 |
|---|---|
| 等宽 / 代码 / 装饰 | JetBrains Mono |
| 正文标题 | -apple-system, BlinkMacSystemFont, sans-serif |
| 正文 body | 同上 |

### 7.3 间距与版式

- 最大内容宽度：720px（首页）/ 1024px（文档正文区）
- Section 垂直间距：24-32px
- 边框圆角：3-8px（不追求大圆角）

### 7.4 主题

首版**仅深色主题**（Terminal Hacker）。浅色主题 v2 再考虑。

---

## 8. 国际化

- 默认 `zh-CN`，可切换 `en`
- 路由策略：`kaku.site/` = 中文，`kaku.site/en/` = 英文
- Starlight 官方 i18n 方案，md 文件按语言分目录
- 语言切换保留当前页面位置

---

## 9. 明确砍掉 / 不做（YAGNI）

| 砍掉 | 原因 |
|---|---|
| `/blog` | 首版无内容可写 |
| `/showcase`（v1） | v1 只官方截图，v2 再开放 UGC |
| 用户账号 / 评论 | 静态站无需动态 |
| Algolia / 外部搜索 | Pagefind 足够 |
| Google Analytics / Plausible | 不收集用户数据 |
| 视频 / WebGL / 3D | 与 Terminal Hacker 调性冲突，GitHub Pages 容量不友好 |
| 浅色主题 | v2 再做 |

---

## 10. 交付范围

### 10.1 v1（首发 MVP）

- [x] `/` 首页 9 个 section
- [x] `/docs` 任务导向入口 + 5 个现有 md 导入归类
- [x] `/download` 下载页
- [x] `/changelog` 从 GitHub Releases 同步
- [x] `/roadmap` 三段式列表
- [x] 全站页脚（4 列）
- [x] 中英双语切换
- [x] 深色主题（Terminal Hacker）
- [x] Pagefind 本地搜索
- [x] GitHub Pages 部署 + 自定义域名 kaku.site（若已购）

### 10.2 v2（迭代）

- [ ] `/showcase` 主题截图廊（开放社区投稿）
- [ ] `/blog`
- [ ] 文档增强组件细化
- [ ] Roadmap 接 GitHub Projects API
- [ ] 浅色主题
- [ ] 更多互动演示（asciinema 内嵌等）

---

## 11. 风险与权衡

| 风险 | 缓解 |
|---|---|
| GitHub Pages 构建时长随文档增多变慢 | Astro 静态构建已经很快，v1 预计几秒内；真慢了再迁 Cloudflare Pages |
| 中英双语维护成本翻倍 | v1 只中文完整、英文机翻 + 关键页人工；README 级别同步 |
| Terminal Hacker 调性过于小众 | 若数据不佳，保留切换浅色主题的退路（v2 再做） |
| Section 2 CSS 动画在 Safari 抖动 | 测试兼容，不行退回 `prefers-reduced-motion` 静态截图 |
| `kaku.site` 域名未购 | 如未购，v1 用 `tw93.github.io/kaku` 先上线 |

---

## 12. 开放问题

本 spec 确认阶段所有核心问题均已拍板。以下可在实施时再决定：

- 自定义域名 `kaku.site` 是否已拥有 / 需购买？
- 字体加载策略：托管 JetBrains Mono 自托管 vs 用 Google Fonts CDN？
- 是否需要 RSS / sitemap.xml（Starlight 默认有 sitemap）

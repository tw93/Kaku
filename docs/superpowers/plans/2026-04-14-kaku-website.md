# Kaku Website Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship v1 of Kaku's official website — a Terminal Hacker–themed landing page plus a Starlight-powered docs site, bilingual (zh-CN primary, en secondary), deployed to GitHub Pages.

**Architecture:** Astro + Starlight static site at `website/` subdirectory inside the Kaku monorepo. Landing page is a single Astro route composed of nine section components. Docs reuse the existing `docs/*.md` content imported into Starlight's content collections. Styling is a dark, monospace, CLI-flavored theme layered on top of Starlight's default CSS. Deployed via GitHub Actions to GitHub Pages. No runtime JavaScript beyond Starlight's defaults + a few scoped `<script>` islands for the terminal demo and tab switcher.

**Tech Stack:** Astro 4.x, Starlight 0.x, Pagefind (built into Starlight), pnpm, TypeScript (Astro default), GitHub Actions, GitHub Pages. Node 20+.

**Spec reference:** `docs/superpowers/specs/2026-04-14-kaku-website-design.md`

**Branch:** Continue on `web-ui` (already checked out). All tasks commit to `web-ui`.

---

## File Structure

All files live under `website/` at the repo root. Existing project files are untouched except `.github/workflows/deploy-website.yml` (new) and the root `.gitignore` (add `website/node_modules`, `website/dist`, `website/.astro`).

```
website/
├── package.json                          # pnpm project, deps: astro, @astrojs/starlight
├── astro.config.mjs                      # Starlight integration, i18n, site URL, base path
├── tsconfig.json                         # Astro strict preset
├── public/
│   ├── favicon.svg                       # simple "K" mark or reuse assets/logo
│   └── og-image.png                      # optional, can be placeholder for v1
├── src/
│   ├── content/
│   │   ├── config.ts                     # defineCollection for docs
│   │   └── docs/
│   │       ├── index.mdx                 # docs landing (task-oriented cards, zh)
│   │       ├── start/
│   │       │   ├── install.md            # stub
│   │       │   ├── first-config.md       # stub
│   │       │   ├── migrate-iterm2.md     # stub
│   │       │   └── migrate-wezterm.md    # stub
│   │       ├── ai/
│   │       │   ├── overview.md           # extracted from features.md
│   │       │   ├── error-recovery.md     # stub
│   │       │   ├── nl-to-command.md      # stub
│   │       │   └── providers.md          # stub
│   │       ├── config/
│   │       │   ├── lua.md                # from configuration.md
│   │       │   ├── theme.md              # stub
│   │       │   ├── font.md               # stub
│   │       │   └── keybindings.md        # from keybindings.md
│   │       ├── features/
│   │       │   ├── tabs-windows.md       # stub
│   │       │   ├── panes-broadcast.md    # stub
│   │       │   ├── integrations.md       # stub
│   │       │   └── shell.md              # stub
│   │       ├── reference/
│   │       │   ├── cli.md                # from cli.md
│   │       │   ├── faq.md                # from faq.md
│   │       │   └── changelog.md          # stub (links to /changelog)
│   │       └── en/                       # English mirror, stubs in v1
│   │           └── index.mdx
│   ├── pages/
│   │   ├── index.astro                   # zh landing
│   │   ├── en/
│   │   │   └── index.astro               # en landing
│   │   ├── download.astro
│   │   ├── roadmap.astro
│   │   └── changelog.astro
│   ├── components/
│   │   ├── landing/
│   │   │   ├── Hero.astro
│   │   │   ├── TerminalDemo.astro        # signature animated demo
│   │   │   ├── FeatureGrid.astro
│   │   │   ├── AIShowcase.astro
│   │   │   ├── MigrateTabs.astro         # iTerm2 / WezTerm tab switcher
│   │   │   ├── ScreenshotGallery.astro
│   │   │   ├── QuickStart.astro
│   │   │   ├── WhyKaku.astro
│   │   │   └── FAQ.astro
│   │   ├── SiteNav.astro                 # global top nav (landing pages)
│   │   └── SiteFooter.astro              # global footer (4 cols)
│   ├── layouts/
│   │   └── LandingLayout.astro           # wraps landing pages, pulls in SiteFooter
│   ├── styles/
│   │   ├── tokens.css                    # color + font tokens (Terminal Hacker)
│   │   ├── global.css                    # base styles
│   │   └── starlight-overrides.css       # dark-only overrides for docs
│   └── content-pages/
│       └── roadmap-data.ts               # three-stage Now/Next/Later list
├── tests/
│   └── smoke.spec.ts                     # Playwright smoke test for landing
├── playwright.config.ts
└── .gitignore                            # node_modules, dist, .astro
```

**Responsibility boundaries:**
- `src/components/landing/*` — each file is one homepage section, no cross-section imports, no data fetching. Pure presentation.
- `src/layouts/LandingLayout.astro` — owns `<html>`, `<head>`, global styles, footer slot.
- `src/styles/tokens.css` — single source of truth for colors, fonts, spacing. Components reference CSS variables, never hard-code colors.
- `src/content-pages/roadmap-data.ts` — typed constant, so the roadmap page is data-driven.

---

## Scope & TDD Posture

This is a content-heavy static marketing site. Applying classical red-green-refactor unit TDD to Astro components is low-value (they're templates + CSS). The testing strategy is:

1. **Build gate** — `pnpm build` must succeed after every task. This catches type errors, broken imports, invalid frontmatter.
2. **Playwright smoke test** — one headless test that loads the landing page and asserts the nine section markers are present and the primary download CTA exists. Re-run at end of Phase 2 and Phase 6.
3. **Manual visual review** — at the end of Phase 2, open the local dev server and walk the homepage. The spec calls out look-and-feel criteria that can't be unit-tested.
4. **Lighthouse check (manual)** — at the end of Phase 6, run Lighthouse in Chrome DevTools against the production build preview, confirm Performance / Accessibility / Best Practices ≥ 95.

So tests appear at phase boundaries, not after every component. This is **pragmatic testing, not TDD theater**.

---

## Phase 0 — Project Scaffold

### Task 0.1: Create Astro + Starlight project

**Files:**
- Create: `website/package.json`
- Create: `website/astro.config.mjs`
- Create: `website/tsconfig.json`
- Create: `website/src/content/config.ts`
- Create: `website/public/favicon.svg`
- Create: `website/.gitignore`
- Modify: `.gitignore` (repo root)

- [ ] **Step 1: Verify Node and pnpm available**

Run: `node --version && pnpm --version`
Expected: Node ≥ 20.0.0, pnpm ≥ 8.0.0. If pnpm missing: `npm install -g pnpm`.

- [ ] **Step 2: Create website directory and package.json**

Run: `mkdir -p website && cd website`

Create `website/package.json`:

```json
{
  "name": "kaku-website",
  "type": "module",
  "version": "0.1.0",
  "private": true,
  "scripts": {
    "dev": "astro dev",
    "build": "astro build",
    "preview": "astro preview",
    "check": "astro check",
    "test:smoke": "playwright test"
  },
  "dependencies": {
    "@astrojs/starlight": "^0.28.0",
    "astro": "^4.16.0",
    "sharp": "^0.33.0"
  },
  "devDependencies": {
    "@playwright/test": "^1.47.0",
    "typescript": "^5.5.0"
  }
}
```

- [ ] **Step 3: Create astro.config.mjs**

Create `website/astro.config.mjs`:

```javascript
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

export default defineConfig({
  site: 'https://tw93.github.io',
  base: '/Kaku',
  integrations: [
    starlight({
      title: 'Kaku',
      description: '为 AI 编码而生的终端',
      logo: { src: './public/favicon.svg' },
      social: {
        github: 'https://github.com/tw93/Kaku',
      },
      defaultLocale: 'root',
      locales: {
        root: { label: '简体中文', lang: 'zh-CN' },
        en: { label: 'English', lang: 'en' },
      },
      customCss: [
        './src/styles/tokens.css',
        './src/styles/starlight-overrides.css',
      ],
      sidebar: [
        {
          label: '快速开始',
          items: [
            { label: '安装', slug: 'start/install' },
            { label: '首次配置', slug: 'start/first-config' },
            { label: '从 iTerm2 迁移', slug: 'start/migrate-iterm2' },
            { label: '从 WezTerm 迁移', slug: 'start/migrate-wezterm' },
          ],
        },
        {
          label: 'AI 功能',
          items: [
            { label: '总览', slug: 'ai/overview' },
            { label: '错误自动修复', slug: 'ai/error-recovery' },
            { label: '自然语言转命令', slug: 'ai/nl-to-command' },
            { label: 'Provider 配置', slug: 'ai/providers' },
          ],
        },
        {
          label: '配置',
          items: [
            { label: 'Lua 配置', slug: 'config/lua' },
            { label: '主题与配色', slug: 'config/theme' },
            { label: '字体与渲染', slug: 'config/font' },
            { label: '快捷键', slug: 'config/keybindings' },
          ],
        },
        {
          label: '功能详解',
          items: [
            { label: '标签与窗口', slug: 'features/tabs-windows' },
            { label: '分屏与广播输入', slug: 'features/panes-broadcast' },
            { label: 'Lazygit / Yazi 集成', slug: 'features/integrations' },
            { label: 'Shell 集成', slug: 'features/shell' },
          ],
        },
        {
          label: '参考',
          items: [
            { label: 'CLI 命令', slug: 'reference/cli' },
            { label: 'FAQ', slug: 'reference/faq' },
            { label: '更新日志', slug: 'reference/changelog' },
          ],
        },
      ],
    }),
  ],
});
```

- [ ] **Step 4: Create tsconfig.json**

Create `website/tsconfig.json`:

```json
{
  "extends": "astro/tsconfigs/strict",
  "include": [".astro/types.d.ts", "**/*"],
  "exclude": ["dist"]
}
```

- [ ] **Step 5: Create content collection config**

Create `website/src/content/config.ts`:

```typescript
import { defineCollection } from 'astro:content';
import { docsSchema } from '@astrojs/starlight/schema';

export const collections = {
  docs: defineCollection({ schema: docsSchema() }),
};
```

- [ ] **Step 6: Create placeholder favicon**

Create `website/public/favicon.svg`:

```xml
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32">
  <rect width="32" height="32" fill="#0a0a0a"/>
  <text x="16" y="23" font-family="monospace" font-size="20" font-weight="700" fill="#00ff9f" text-anchor="middle">K</text>
</svg>
```

- [ ] **Step 7: Create website/.gitignore**

Create `website/.gitignore`:

```
node_modules/
dist/
.astro/
.DS_Store
test-results/
playwright-report/
```

- [ ] **Step 8: Update repo root .gitignore**

Add to root `/Users/darion.yaphet/source/Kaku/.gitignore` (append):

```
website/node_modules/
website/dist/
website/.astro/
website/test-results/
website/playwright-report/
```

- [ ] **Step 9: Install dependencies**

Run: `cd website && pnpm install`
Expected: completes without errors, creates `website/node_modules` and `website/pnpm-lock.yaml`.

- [ ] **Step 10: Create a minimum docs index so the build succeeds**

Create `website/src/content/docs/index.mdx`:

```mdx
---
title: Kaku 文档
description: 为 AI 编码而生的终端
template: splash
---

# Kaku 文档（placeholder）

文档内容将在后续任务中导入。
```

- [ ] **Step 11: Verify build passes**

Run: `cd website && pnpm build`
Expected: build succeeds, `website/dist/` contains generated HTML.

- [ ] **Step 12: Commit**

```bash
git add website/ .gitignore
git commit -m "feat(website): scaffold astro + starlight project"
```

---

### Task 0.2: Add Terminal Hacker design tokens

**Files:**
- Create: `website/src/styles/tokens.css`
- Create: `website/src/styles/global.css`
- Create: `website/src/styles/starlight-overrides.css`

- [ ] **Step 1: Create tokens.css**

Create `website/src/styles/tokens.css`:

```css
:root {
  /* Colors — Terminal Hacker */
  --kk-bg: #0a0a0a;
  --kk-bg-elev: #050505;
  --kk-bg-card: #111111;
  --kk-border: #1f1f1f;
  --kk-border-strong: #2a2a2a;
  --kk-text: #e5e5e5;
  --kk-text-dim: #888888;
  --kk-text-muted: #666666;
  --kk-accent: #00ff9f;
  --kk-accent-dim: #00b870;
  --kk-error: #ff5c5c;
  --kk-warn: #ffd56b;

  /* Typography */
  --kk-font-mono: 'JetBrains Mono', ui-monospace, 'SF Mono', Menlo, Consolas, monospace;
  --kk-font-sans: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;

  /* Spacing */
  --kk-space-1: 4px;
  --kk-space-2: 8px;
  --kk-space-3: 12px;
  --kk-space-4: 16px;
  --kk-space-6: 24px;
  --kk-space-8: 32px;
  --kk-space-12: 48px;
  --kk-space-16: 64px;

  /* Radius */
  --kk-radius-sm: 3px;
  --kk-radius-md: 6px;
  --kk-radius-lg: 10px;

  /* Content widths */
  --kk-max-narrow: 720px;
  --kk-max-wide: 1024px;
}

/* Force dark-only for v1. Light mode ships in v2. */
:root[data-theme='light'] {
  color-scheme: dark;
}
```

- [ ] **Step 2: Create global.css**

Create `website/src/styles/global.css`:

```css
@import './tokens.css';

html, body {
  margin: 0;
  padding: 0;
  background: var(--kk-bg);
  color: var(--kk-text);
  font-family: var(--kk-font-sans);
  -webkit-font-smoothing: antialiased;
  -moz-osx-font-smoothing: grayscale;
}

* {
  box-sizing: border-box;
}

code, kbd, pre {
  font-family: var(--kk-font-mono);
}

a {
  color: var(--kk-accent);
  text-decoration: none;
}

a:hover {
  text-decoration: underline;
}

.kk-container {
  max-width: var(--kk-max-narrow);
  margin: 0 auto;
  padding: 0 var(--kk-space-6);
}

.kk-container-wide {
  max-width: var(--kk-max-wide);
  margin: 0 auto;
  padding: 0 var(--kk-space-6);
}

@media (prefers-reduced-motion: reduce) {
  *,
  *::before,
  *::after {
    animation-duration: 0.001ms !important;
    animation-iteration-count: 1 !important;
    transition-duration: 0.001ms !important;
  }
}
```

- [ ] **Step 3: Create starlight-overrides.css**

Create `website/src/styles/starlight-overrides.css`:

```css
/* Force Starlight into dark-only for v1. */
:root, :root[data-theme='dark'], :root[data-theme='light'] {
  --sl-color-bg: #0a0a0a;
  --sl-color-bg-nav: #050505;
  --sl-color-bg-sidebar: #050505;
  --sl-color-text: #e5e5e5;
  --sl-color-text-accent: #00ff9f;
  --sl-color-accent: #00ff9f;
  --sl-color-accent-high: #00ff9f;
  --sl-color-hairline: #1f1f1f;
  --sl-color-hairline-light: #1f1f1f;
  --sl-font: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
  --sl-font-mono: 'JetBrains Mono', ui-monospace, 'SF Mono', Menlo, monospace;
}

/* Hide theme selector since we're dark-only in v1 */
starlight-theme-select { display: none !important; }
```

- [ ] **Step 4: Verify build still passes**

Run: `cd website && pnpm build`
Expected: PASS, no CSS errors.

- [ ] **Step 5: Commit**

```bash
git add website/src/styles/
git commit -m "feat(website): add terminal hacker design tokens and starlight overrides"
```

---

## Phase 1 — Global Layout & Footer

### Task 1.1: Build SiteFooter component

**Files:**
- Create: `website/src/components/SiteFooter.astro`
- Create: `website/src/layouts/LandingLayout.astro`

- [ ] **Step 1: Create SiteFooter.astro**

Create `website/src/components/SiteFooter.astro`:

```astro
---
const year = new Date().getFullYear();

interface LinkGroup {
  title: string;
  links: { label: string; href: string }[];
}

const groups: LinkGroup[] = [
  {
    title: '产品',
    links: [
      { label: '下载', href: '/Kaku/download' },
      { label: 'Changelog', href: '/Kaku/changelog' },
      { label: '主题', href: '/Kaku/docs/config/theme' },
    ],
  },
  {
    title: '文档',
    links: [
      { label: '快速开始', href: '/Kaku/docs/start/install' },
      { label: 'AI 功能', href: '/Kaku/docs/ai/overview' },
      { label: '配置', href: '/Kaku/docs/config/lua' },
      { label: 'FAQ', href: '/Kaku/docs/reference/faq' },
    ],
  },
  {
    title: '社区',
    links: [
      { label: 'GitHub', href: 'https://github.com/tw93/Kaku' },
      { label: 'Issues', href: 'https://github.com/tw93/Kaku/issues' },
      { label: 'X / Twitter', href: 'https://twitter.com/HiTw93' },
    ],
  },
  {
    title: '项目',
    links: [
      { label: 'Roadmap', href: '/Kaku/roadmap' },
      { label: 'License', href: 'https://github.com/tw93/Kaku/blob/main/LICENSE.md' },
      { label: '作者', href: 'https://github.com/tw93' },
    ],
  },
];
---

<footer class="site-footer">
  <div class="kk-container-wide footer-inner">
    <div class="footer-cols">
      {groups.map(group => (
        <div class="footer-col">
          <h4>{group.title}</h4>
          <ul>
            {group.links.map(link => (
              <li><a href={link.href}>{link.label}</a></li>
            ))}
          </ul>
        </div>
      ))}
    </div>
    <div class="footer-bottom">
      <span>Kaku · Built on <a href="https://github.com/wez/wezterm">WezTerm</a> · MIT License · © {year} Tw93</span>
    </div>
  </div>
</footer>

<style>
  .site-footer {
    background: var(--kk-bg-elev);
    border-top: 1px solid var(--kk-border);
    padding: var(--kk-space-12) 0 var(--kk-space-6);
    margin-top: var(--kk-space-16);
  }
  .footer-inner { display: flex; flex-direction: column; gap: var(--kk-space-8); }
  .footer-cols {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: var(--kk-space-6);
  }
  .footer-col h4 {
    color: var(--kk-text);
    font-size: 12px;
    font-family: var(--kk-font-mono);
    text-transform: uppercase;
    letter-spacing: 1px;
    margin: 0 0 var(--kk-space-3);
  }
  .footer-col ul { list-style: none; padding: 0; margin: 0; }
  .footer-col li { margin-bottom: var(--kk-space-2); }
  .footer-col a {
    color: var(--kk-text-dim);
    font-size: 13px;
  }
  .footer-col a:hover { color: var(--kk-accent); }
  .footer-bottom {
    border-top: 1px dashed var(--kk-border);
    padding-top: var(--kk-space-4);
    font-size: 12px;
    color: var(--kk-text-muted);
    font-family: var(--kk-font-mono);
  }
  .footer-bottom a { color: var(--kk-text-dim); }
  @media (max-width: 720px) {
    .footer-cols { grid-template-columns: repeat(2, 1fr); }
  }
</style>
```

- [ ] **Step 2: Create LandingLayout.astro**

Create `website/src/layouts/LandingLayout.astro`:

```astro
---
import SiteNav from '../components/SiteNav.astro';
import SiteFooter from '../components/SiteFooter.astro';
import '../styles/global.css';

interface Props {
  title: string;
  description: string;
  lang?: string;
}
const { title, description, lang = 'zh-CN' } = Astro.props;
---

<!DOCTYPE html>
<html lang={lang}>
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>{title}</title>
    <meta name="description" content={description} />
    <link rel="icon" type="image/svg+xml" href="/Kaku/favicon.svg" />
  </head>
  <body>
    <SiteNav lang={lang} />
    <slot />
    <SiteFooter />
  </body>
</html>
```

Note: `SiteNav` is created in the next task. Build will fail until then — that's expected.

- [ ] **Step 3: Commit (build verified in next task after SiteNav lands)**

```bash
git add website/src/components/SiteFooter.astro website/src/layouts/LandingLayout.astro
git commit -m "feat(website): add site footer and landing layout shell"
```

---

### Task 1.2: Build SiteNav component

**Files:**
- Create: `website/src/components/SiteNav.astro`

- [ ] **Step 1: Create SiteNav.astro**

Create `website/src/components/SiteNav.astro`:

```astro
---
interface Props {
  lang?: string;
}
const { lang = 'zh-CN' } = Astro.props;
const isEn = lang === 'en';

interface NavLink { label: string; href: string; }
const links: NavLink[] = isEn ? [
  { label: 'Docs',      href: '/Kaku/en/docs/' },
  { label: 'Download',  href: '/Kaku/download' },
  { label: 'Roadmap',   href: '/Kaku/roadmap' },
  { label: 'Changelog', href: '/Kaku/changelog' },
] : [
  { label: '文档',      href: '/Kaku/docs/' },
  { label: '下载',      href: '/Kaku/download' },
  { label: 'Roadmap',   href: '/Kaku/roadmap' },
  { label: 'Changelog', href: '/Kaku/changelog' },
];

const langSwitchHref = isEn ? '/Kaku/' : '/Kaku/en/';
const langSwitchLabel = isEn ? '中' : 'EN';
---

<nav class="site-nav">
  <div class="kk-container-wide nav-inner">
    <a class="brand" href={isEn ? '/Kaku/en/' : '/Kaku/'}>
      <span class="brand-mark">▸</span>
      <span class="brand-name">Kaku</span>
    </a>
    <div class="nav-links">
      {links.map(link => (
        <a class="nav-link" href={link.href}>{link.label}</a>
      ))}
    </div>
    <div class="nav-right">
      <a class="lang-switch" href={langSwitchHref}>{langSwitchLabel}</a>
      <a class="gh-link" href="https://github.com/tw93/Kaku" target="_blank" rel="noopener">
        <span>GitHub</span>
        <span class="star">★</span>
      </a>
    </div>
  </div>
</nav>

<style>
  .site-nav {
    background: var(--kk-bg-elev);
    border-bottom: 1px solid var(--kk-border);
    padding: var(--kk-space-3) 0;
    position: sticky;
    top: 0;
    z-index: 100;
    backdrop-filter: blur(8px);
  }
  .nav-inner {
    display: flex;
    align-items: center;
    gap: var(--kk-space-6);
  }
  .brand {
    display: flex;
    align-items: center;
    gap: var(--kk-space-2);
    text-decoration: none;
    font-family: var(--kk-font-mono);
    font-weight: 700;
    color: var(--kk-text);
  }
  .brand-mark { color: var(--kk-accent); }
  .brand-name { font-size: 16px; }
  .nav-links {
    display: flex;
    gap: var(--kk-space-6);
    flex: 1;
  }
  .nav-link {
    color: var(--kk-text-dim);
    font-size: 13px;
    text-decoration: none;
    font-family: var(--kk-font-mono);
  }
  .nav-link:hover { color: var(--kk-accent); text-decoration: none; }
  .nav-right {
    display: flex;
    align-items: center;
    gap: var(--kk-space-4);
  }
  .lang-switch {
    color: var(--kk-text-dim);
    font-family: var(--kk-font-mono);
    font-size: 12px;
    padding: var(--kk-space-1) var(--kk-space-2);
    border: 1px solid var(--kk-border-strong);
    border-radius: var(--kk-radius-sm);
    text-decoration: none;
  }
  .lang-switch:hover { color: var(--kk-accent); border-color: var(--kk-accent); text-decoration: none; }
  .gh-link {
    display: flex;
    align-items: center;
    gap: var(--kk-space-2);
    color: var(--kk-text);
    font-family: var(--kk-font-mono);
    font-size: 12px;
    padding: var(--kk-space-1) var(--kk-space-3);
    border: 1px solid var(--kk-border-strong);
    border-radius: var(--kk-radius-sm);
    text-decoration: none;
  }
  .gh-link:hover { border-color: var(--kk-accent); text-decoration: none; }
  .gh-link .star { color: var(--kk-accent); }
  @media (max-width: 720px) {
    .nav-links { display: none; }
  }
</style>
```

- [ ] **Step 2: Build**

Run: `cd website && pnpm build`
Expected: PASS. The `LandingLayout` → `SiteNav` import chain now resolves.

- [ ] **Step 3: Commit**

```bash
git add website/src/components/SiteNav.astro
git commit -m "feat(website): add site nav with lang switch and github link"
```

---

## Phase 2 — Landing Page Components

Each task in this phase creates one homepage section component. Commit after every component so that `pnpm build` stays green.

### Task 2.1: Hero component

**Files:**
- Create: `website/src/components/landing/Hero.astro`

- [ ] **Step 1: Create Hero.astro**

Create `website/src/components/landing/Hero.astro`:

```astro
---
interface Props {
  title: string;
  subtitle: string;
  downloadLabel: string;
  brewCommand: string;
  stars: string;
}
const { title, subtitle, downloadLabel, brewCommand, stars } = Astro.props;
---

<section class="hero">
  <div class="kk-container">
    <div class="prompt">~/kaku $ launch</div>
    <h1>{title}</h1>
    <p class="subtitle">{subtitle}</p>
    <div class="ctas">
      <a class="cta cta-primary" href="/Kaku/download">
        <span class="arrow">↓</span> {downloadLabel}
      </a>
      <button class="cta cta-ghost" data-copy={brewCommand}>
        <code>{brewCommand}</code>
        <span class="copy-hint">复制</span>
      </button>
    </div>
    <div class="stats">
      <span class="stat"><span class="num">★ {stars}</span></span>
      <span class="sep">·</span>
      <span class="stat"><span class="num">40% 更小</span></span>
      <span class="sep">·</span>
      <span class="stat"><span class="num">macOS · Linux</span></span>
    </div>
  </div>
</section>

<script>
  document.querySelectorAll('button[data-copy]').forEach(btn => {
    btn.addEventListener('click', async () => {
      const text = (btn as HTMLButtonElement).dataset.copy;
      if (!text) return;
      try {
        await navigator.clipboard.writeText(text);
        const hint = btn.querySelector('.copy-hint');
        if (hint) {
          const original = hint.textContent;
          hint.textContent = '✓ 已复制';
          setTimeout(() => { hint.textContent = original; }, 1500);
        }
      } catch {}
    });
  });
</script>

<style>
  .hero {
    padding: var(--kk-space-16) 0 var(--kk-space-12);
    background: var(--kk-bg-elev);
    border-bottom: 1px dashed var(--kk-border);
    text-align: center;
  }
  .prompt {
    color: var(--kk-accent);
    font-family: var(--kk-font-mono);
    font-size: 13px;
    opacity: 0.7;
    margin-bottom: var(--kk-space-4);
  }
  h1 {
    color: var(--kk-text);
    font-size: 48px;
    line-height: 1.1;
    letter-spacing: -1px;
    margin: 0 0 var(--kk-space-4);
    font-weight: 700;
  }
  .subtitle {
    color: var(--kk-text-dim);
    font-size: 16px;
    margin: 0 0 var(--kk-space-8);
  }
  .ctas {
    display: flex;
    gap: var(--kk-space-3);
    justify-content: center;
    flex-wrap: wrap;
    margin-bottom: var(--kk-space-8);
  }
  .cta {
    display: inline-flex;
    align-items: center;
    gap: var(--kk-space-2);
    padding: var(--kk-space-3) var(--kk-space-6);
    border-radius: var(--kk-radius-sm);
    font-family: var(--kk-font-mono);
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
    border: 1px solid transparent;
    text-decoration: none;
  }
  .cta-primary {
    background: var(--kk-accent);
    color: var(--kk-bg);
  }
  .cta-primary:hover { background: var(--kk-accent-dim); text-decoration: none; }
  .cta-ghost {
    background: transparent;
    color: var(--kk-text);
    border-color: var(--kk-border-strong);
  }
  .cta-ghost code { background: none; color: inherit; font-size: 13px; }
  .cta-ghost .copy-hint { color: var(--kk-text-muted); font-size: 11px; }
  .stats {
    display: flex;
    gap: var(--kk-space-3);
    justify-content: center;
    font-size: 12px;
    color: var(--kk-text-muted);
    font-family: var(--kk-font-mono);
  }
  .stats .num { color: var(--kk-accent); font-weight: 600; }
  .sep { color: var(--kk-text-muted); }
  @media (max-width: 720px) {
    h1 { font-size: 32px; }
  }
</style>
```

- [ ] **Step 2: Build**

Run: `cd website && pnpm build`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add website/src/components/landing/Hero.astro
git commit -m "feat(website): add hero section"
```

---

### Task 2.2: TerminalDemo component (signature animation)

**Files:**
- Create: `website/src/components/landing/TerminalDemo.astro`

- [ ] **Step 1: Create TerminalDemo.astro**

Create `website/src/components/landing/TerminalDemo.astro`:

```astro
---
// Pure CSS animation. No JavaScript needed.
// Respects prefers-reduced-motion via global.css.
---

<section class="td-section">
  <div class="kk-container">
    <div class="td-tag">[2] 错误自动修复</div>
    <div class="terminal">
      <div class="term-bar">
        <span class="dot red"></span>
        <span class="dot yellow"></span>
        <span class="dot green"></span>
        <span class="term-title">~/proj — kaku</span>
      </div>
      <div class="term-body">
        <div class="line line-1">
          <span class="prompt">~/proj $</span>
          <span class="cmd">npm run buidl</span>
          <span class="cursor">▌</span>
        </div>
        <div class="line line-2 err">npm ERR! Missing script: "buidl"</div>
        <div class="line line-3 hint">
          <span class="icon">💡</span>
          <span>Kaku: Did you mean <code>npm run build</code>? Press <kbd>⌘⇧E</kbd> to apply</span>
        </div>
        <div class="line line-4">
          <span class="prompt">~/proj $</span>
          <span class="cmd">npm run build</span>
          <span class="ok">✓</span>
        </div>
      </div>
    </div>
    <h2 class="td-title">一个会自己动的终端演示</h2>
    <p class="td-desc">Kaku 检测到命令失败后自动建议修复，按 <kbd>⌘⇧E</kbd> 一键应用。这是 Kaku 最具差异化的卖点之一。</p>
  </div>
</section>

<style>
  .td-section {
    padding: var(--kk-space-16) 0;
    border-bottom: 1px dashed var(--kk-border);
  }
  .td-tag {
    color: var(--kk-accent);
    font-family: var(--kk-font-mono);
    font-size: 11px;
    letter-spacing: 1px;
    opacity: 0.7;
    margin-bottom: var(--kk-space-3);
    text-align: center;
  }
  .terminal {
    background: #000;
    border: 1px solid var(--kk-border-strong);
    border-radius: var(--kk-radius-md);
    overflow: hidden;
    box-shadow: 0 20px 60px rgba(0, 255, 159, 0.05);
    max-width: 640px;
    margin: 0 auto var(--kk-space-8);
  }
  .term-bar {
    background: #111;
    padding: var(--kk-space-2) var(--kk-space-4);
    display: flex;
    align-items: center;
    gap: var(--kk-space-2);
    border-bottom: 1px solid var(--kk-border);
  }
  .dot { width: 10px; height: 10px; border-radius: 50%; }
  .dot.red { background: #ff5f57; }
  .dot.yellow { background: #febc2e; }
  .dot.green { background: #28c840; }
  .term-title {
    color: var(--kk-text-muted);
    font-size: 11px;
    font-family: var(--kk-font-mono);
    margin-left: auto;
  }
  .term-body {
    padding: var(--kk-space-6);
    font-family: var(--kk-font-mono);
    font-size: 13px;
    min-height: 160px;
  }
  .line { margin-bottom: var(--kk-space-2); opacity: 0; animation: appear 12s infinite; }
  .line-1 { animation-delay: 0s; color: var(--kk-text); }
  .line-2 { animation-delay: 2s; color: var(--kk-error); }
  .line-3 { animation-delay: 3.5s; color: var(--kk-text-dim); }
  .line-4 { animation-delay: 6s; color: var(--kk-text); }
  .prompt { color: var(--kk-accent); margin-right: var(--kk-space-2); }
  .cmd { color: var(--kk-text); }
  .ok { color: var(--kk-accent); margin-left: var(--kk-space-2); }
  .icon { margin-right: var(--kk-space-2); }
  .line code {
    background: rgba(0, 255, 159, 0.1);
    color: var(--kk-accent);
    padding: 1px 4px;
    border-radius: 2px;
  }
  .line kbd {
    background: var(--kk-bg-card);
    color: var(--kk-text);
    padding: 1px 5px;
    border-radius: 3px;
    border: 1px solid var(--kk-border-strong);
    font-size: 11px;
  }
  .cursor {
    display: inline-block;
    color: var(--kk-accent);
    animation: blink 1s steps(2) infinite;
  }
  @keyframes appear {
    0% { opacity: 0; transform: translateY(4px); }
    5%, 70% { opacity: 1; transform: translateY(0); }
    80%, 100% { opacity: 0; }
  }
  @keyframes blink {
    0%, 50% { opacity: 1; }
    51%, 100% { opacity: 0; }
  }
  .td-title {
    color: var(--kk-text);
    font-size: 24px;
    text-align: center;
    margin: 0 0 var(--kk-space-3);
  }
  .td-desc {
    color: var(--kk-text-dim);
    font-size: 14px;
    text-align: center;
    max-width: 520px;
    margin: 0 auto;
  }
  .td-desc code {
    background: var(--kk-bg-card);
    color: var(--kk-accent);
    padding: 1px 5px;
    border-radius: 3px;
    font-size: 12px;
  }
  .td-desc kbd {
    background: var(--kk-bg-card);
    color: var(--kk-text);
    padding: 1px 5px;
    border-radius: 3px;
    border: 1px solid var(--kk-border-strong);
    font-size: 12px;
  }
</style>
```

- [ ] **Step 2: Build**

Run: `cd website && pnpm build`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add website/src/components/landing/TerminalDemo.astro
git commit -m "feat(website): add terminal demo section with css animation"
```

---

### Task 2.3: FeatureGrid component

**Files:**
- Create: `website/src/components/landing/FeatureGrid.astro`

- [ ] **Step 1: Create FeatureGrid.astro**

Create `website/src/components/landing/FeatureGrid.astro`:

```astro
---
interface Feature {
  icon: string;
  title: string;
  desc: string;
  href: string;
}

const features: Feature[] = [
  { icon: '▸', title: '零配置',       desc: 'JetBrains Mono 默认，开箱即用', href: '/Kaku/docs/start/first-config' },
  { icon: '▸', title: 'AI 内建',      desc: '错误修复 / NL 转命令',          href: '/Kaku/docs/ai/overview' },
  { icon: '▸', title: '主题感知',      desc: '跟随系统明暗自动切换',          href: '/Kaku/docs/config/theme' },
  { icon: '▸', title: 'GPU 渲染',     desc: '继承 WezTerm 渲染栈',           href: '/Kaku/docs/config/font' },
  { icon: '▸', title: 'Lua 兼容',     desc: 'WezTerm 配置零迁移',            href: '/Kaku/docs/config/lua' },
  { icon: '▸', title: '集成全家桶',    desc: 'Lazygit / Yazi / Zsh',          href: '/Kaku/docs/features/integrations' },
];
---

<section class="fg-section">
  <div class="kk-container-wide">
    <div class="fg-tag">[3] 6 核心特性</div>
    <div class="grid">
      {features.map(f => (
        <a class="cell" href={f.href}>
          <span class="icon">{f.icon}</span>
          <strong>{f.title}</strong>
          <span class="desc">{f.desc}</span>
        </a>
      ))}
    </div>
  </div>
</section>

<style>
  .fg-section {
    padding: var(--kk-space-16) 0;
    border-bottom: 1px dashed var(--kk-border);
  }
  .fg-tag {
    color: var(--kk-accent);
    font-family: var(--kk-font-mono);
    font-size: 11px;
    letter-spacing: 1px;
    opacity: 0.7;
    margin-bottom: var(--kk-space-6);
    text-align: center;
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: var(--kk-space-3);
    max-width: 960px;
    margin: 0 auto;
  }
  .cell {
    background: var(--kk-bg-card);
    border: 1px solid var(--kk-border);
    border-radius: var(--kk-radius-md);
    padding: var(--kk-space-6);
    display: flex;
    flex-direction: column;
    gap: var(--kk-space-2);
    transition: border-color 0.15s, transform 0.15s;
    text-decoration: none;
  }
  .cell:hover {
    border-color: var(--kk-accent);
    transform: translateY(-2px);
    text-decoration: none;
  }
  .cell .icon {
    color: var(--kk-accent);
    font-family: var(--kk-font-mono);
    font-size: 14px;
  }
  .cell strong {
    color: var(--kk-text);
    font-size: 16px;
    font-weight: 600;
  }
  .cell .desc {
    color: var(--kk-text-dim);
    font-size: 13px;
  }
  @media (max-width: 720px) {
    .grid { grid-template-columns: repeat(2, 1fr); }
  }
</style>
```

- [ ] **Step 2: Build**

Run: `cd website && pnpm build`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add website/src/components/landing/FeatureGrid.astro
git commit -m "feat(website): add feature grid section"
```

---

### Task 2.4: AIShowcase component

**Files:**
- Create: `website/src/components/landing/AIShowcase.astro`

- [ ] **Step 1: Create AIShowcase.astro**

Create `website/src/components/landing/AIShowcase.astro`:

```astro
---
---

<section class="ai-section">
  <div class="kk-container">
    <div class="ai-tag">[4] 自然语言 → 命令</div>
    <div class="ai-box">
      <div class="row"><span class="muted"># 把今天的修改推到 feature 分支</span></div>
      <div class="row arrow">↓ Kaku 自动生成</div>
      <div class="row cmd">git add . &amp;&amp; git commit -m "..." &amp;&amp; git push -u origin feature</div>
    </div>
    <h2>自然语言 → 命令</h2>
    <p>
      在 shell 提示符前打 <code>#</code> 加自然语言，Kaku 调用 LLM 生成命令注入回提示符，
      审查后按回车执行。配合 <kbd>⌘⇧A</kbd> 打开 AI 面板、<kbd>⌘⇧E</kbd> 应用修复建议。
    </p>
  </div>
</section>

<style>
  .ai-section {
    padding: var(--kk-space-16) 0;
    border-bottom: 1px dashed var(--kk-border);
  }
  .ai-tag {
    color: var(--kk-accent);
    font-family: var(--kk-font-mono);
    font-size: 11px;
    letter-spacing: 1px;
    opacity: 0.7;
    margin-bottom: var(--kk-space-6);
    text-align: center;
  }
  .ai-box {
    background: #0d1a12;
    border: 1px solid #1a3a25;
    border-radius: var(--kk-radius-md);
    padding: var(--kk-space-6);
    font-family: var(--kk-font-mono);
    font-size: 13px;
    max-width: 520px;
    margin: 0 auto var(--kk-space-8);
  }
  .row { margin-bottom: var(--kk-space-2); }
  .row:last-child { margin-bottom: 0; }
  .muted { color: var(--kk-accent); }
  .arrow { color: var(--kk-text-dim); font-size: 11px; }
  .cmd { color: var(--kk-text); }
  h2 {
    color: var(--kk-text);
    font-size: 24px;
    text-align: center;
    margin: 0 0 var(--kk-space-3);
  }
  p {
    color: var(--kk-text-dim);
    font-size: 14px;
    text-align: center;
    max-width: 520px;
    margin: 0 auto;
  }
  p code {
    background: var(--kk-bg-card);
    color: var(--kk-accent);
    padding: 1px 5px;
    border-radius: 3px;
    font-size: 12px;
  }
  p kbd {
    background: var(--kk-bg-card);
    color: var(--kk-text);
    padding: 1px 5px;
    border-radius: 3px;
    border: 1px solid var(--kk-border-strong);
    font-size: 12px;
  }
</style>
```

- [ ] **Step 2: Build**

Run: `cd website && pnpm build`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add website/src/components/landing/AIShowcase.astro
git commit -m "feat(website): add ai showcase section"
```

---

### Task 2.5: MigrateTabs component (iTerm2 / WezTerm)

**Files:**
- Create: `website/src/components/landing/MigrateTabs.astro`

- [ ] **Step 1: Create MigrateTabs.astro**

Create `website/src/components/landing/MigrateTabs.astro`:

```astro
---
---

<section class="mt-section">
  <div class="kk-container">
    <div class="mt-tag">[5] 迁移指南</div>
    <h2>从 iTerm2 或 WezTerm 迁移</h2>
    <div class="tabs" role="tablist">
      <button class="tab active" role="tab" data-tab="iterm2">我是 iTerm2 用户</button>
      <button class="tab" role="tab" data-tab="wezterm">我是 WezTerm 用户</button>
    </div>
    <div class="tab-panel active" data-panel="iterm2">
      <ul>
        <li>✓ 保留你熟悉的 <kbd>⌘T</kbd> / <kbd>⌘W</kbd> / <kbd>⌘D</kbd> 习惯</li>
        <li>✓ 启动更快，二进制体积更小</li>
        <li>✓ 额外送你一套 AI 助手（错误修复 + 自然语言转命令）</li>
        <li>✓ GPU 渲染，滚动大输出不卡</li>
      </ul>
      <a class="more" href="/Kaku/docs/start/migrate-iterm2">完整 iTerm2 迁移指南 →</a>
    </div>
    <div class="tab-panel" data-panel="wezterm">
      <ul>
        <li>✓ 你的 <code>.wezterm.lua</code> 配置零迁移直接可用</li>
        <li>✓ 二进制体积比上游小 40%</li>
        <li>✓ 默认字体、配色、字体渲染都更好看</li>
        <li>✓ 内建 AI 功能是上游没有的</li>
      </ul>
      <a class="more" href="/Kaku/docs/start/migrate-wezterm">完整 WezTerm 迁移指南 →</a>
    </div>
  </div>
</section>

<script>
  const tabs = document.querySelectorAll<HTMLButtonElement>('.mt-section .tab');
  const panels = document.querySelectorAll<HTMLDivElement>('.mt-section .tab-panel');
  tabs.forEach(tab => {
    tab.addEventListener('click', () => {
      const target = tab.dataset.tab;
      tabs.forEach(t => t.classList.toggle('active', t === tab));
      panels.forEach(p => p.classList.toggle('active', p.dataset.panel === target));
    });
  });
</script>

<style>
  .mt-section {
    padding: var(--kk-space-16) 0;
    border-bottom: 1px dashed var(--kk-border);
  }
  .mt-tag {
    color: var(--kk-accent);
    font-family: var(--kk-font-mono);
    font-size: 11px;
    letter-spacing: 1px;
    opacity: 0.7;
    margin-bottom: var(--kk-space-3);
    text-align: center;
  }
  h2 {
    color: var(--kk-text);
    font-size: 24px;
    text-align: center;
    margin: 0 0 var(--kk-space-8);
  }
  .tabs {
    display: flex;
    justify-content: center;
    gap: var(--kk-space-2);
    margin-bottom: var(--kk-space-6);
  }
  .tab {
    background: transparent;
    border: 1px solid var(--kk-border-strong);
    color: var(--kk-text-dim);
    padding: var(--kk-space-2) var(--kk-space-4);
    font-family: var(--kk-font-mono);
    font-size: 12px;
    border-radius: var(--kk-radius-sm);
    cursor: pointer;
  }
  .tab.active {
    background: var(--kk-accent);
    color: var(--kk-bg);
    border-color: var(--kk-accent);
  }
  .tab-panel {
    display: none;
    background: var(--kk-bg-card);
    border: 1px solid var(--kk-border);
    border-radius: var(--kk-radius-md);
    padding: var(--kk-space-6);
  }
  .tab-panel.active { display: block; }
  .tab-panel ul { margin: 0; padding-left: var(--kk-space-6); }
  .tab-panel li {
    color: var(--kk-text);
    font-size: 14px;
    margin-bottom: var(--kk-space-2);
  }
  .tab-panel li code, .tab-panel li kbd {
    background: var(--kk-bg);
    color: var(--kk-accent);
    padding: 1px 5px;
    border-radius: 3px;
    font-size: 12px;
  }
  .more {
    display: inline-block;
    margin-top: var(--kk-space-4);
    color: var(--kk-accent);
    font-family: var(--kk-font-mono);
    font-size: 13px;
  }
</style>
```

- [ ] **Step 2: Build**

Run: `cd website && pnpm build`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add website/src/components/landing/MigrateTabs.astro
git commit -m "feat(website): add iterm2/wezterm migrate tabs section"
```

---

### Task 2.6: ScreenshotGallery component

**Files:**
- Create: `website/src/components/landing/ScreenshotGallery.astro`

- [ ] **Step 1: Create ScreenshotGallery.astro**

Create `website/src/components/landing/ScreenshotGallery.astro`:

```astro
---
// v1 uses 6 placeholder screenshots from the existing assets/ folder.
// Replace src paths in a later visual-polish task.
interface Shot {
  src: string;
  alt: string;
}
const shots: Shot[] = [
  { src: '/Kaku/shots/dark-1.jpg', alt: 'Kaku 深色主题 - 主界面' },
  { src: '/Kaku/shots/dark-2.jpg', alt: 'Kaku 深色主题 - 分屏' },
  { src: '/Kaku/shots/dark-3.jpg', alt: 'Kaku 深色主题 - AI 面板' },
  { src: '/Kaku/shots/light-1.jpg', alt: 'Kaku 浅色主题 - 主界面' },
  { src: '/Kaku/shots/light-2.jpg', alt: 'Kaku 浅色主题 - Lazygit' },
  { src: '/Kaku/shots/light-3.jpg', alt: 'Kaku 浅色主题 - Yazi' },
];
---

<section class="sg-section">
  <div class="kk-container-wide">
    <div class="sg-tag">[6] 主题与截图</div>
    <h2>深浅两套主题，都很好看</h2>
    <div class="gallery">
      {shots.map(shot => (
        <figure class="shot">
          <img src={shot.src} alt={shot.alt} loading="lazy" />
        </figure>
      ))}
    </div>
  </div>
</section>

<style>
  .sg-section {
    padding: var(--kk-space-16) 0;
    border-bottom: 1px dashed var(--kk-border);
  }
  .sg-tag {
    color: var(--kk-accent);
    font-family: var(--kk-font-mono);
    font-size: 11px;
    letter-spacing: 1px;
    opacity: 0.7;
    margin-bottom: var(--kk-space-3);
    text-align: center;
  }
  h2 {
    color: var(--kk-text);
    font-size: 24px;
    text-align: center;
    margin: 0 0 var(--kk-space-8);
  }
  .gallery {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: var(--kk-space-3);
    max-width: 960px;
    margin: 0 auto;
  }
  .shot {
    margin: 0;
    background: var(--kk-bg-card);
    border: 1px solid var(--kk-border);
    border-radius: var(--kk-radius-md);
    overflow: hidden;
    aspect-ratio: 16 / 10;
  }
  .shot img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }
  @media (max-width: 720px) {
    .gallery { grid-template-columns: repeat(2, 1fr); }
  }
</style>
```

- [ ] **Step 2: Create placeholder image stubs**

For v1 use the existing `assets/kaku.jpg` as a stand-in for all six slots so the build doesn't 404. Real screenshots are a content-polish task (out of plan scope).

Run:

```bash
mkdir -p website/public/shots
cp assets/kaku.jpg website/public/shots/dark-1.jpg
cp assets/kaku.jpg website/public/shots/dark-2.jpg
cp assets/kaku.jpg website/public/shots/dark-3.jpg
cp assets/kaku.jpg website/public/shots/light-1.jpg
cp assets/kaku.jpg website/public/shots/light-2.jpg
cp assets/kaku.jpg website/public/shots/light-3.jpg
```

- [ ] **Step 3: Build**

Run: `cd website && pnpm build`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add website/src/components/landing/ScreenshotGallery.astro website/public/shots/
git commit -m "feat(website): add screenshot gallery section with placeholders"
```

---

### Task 2.7: QuickStart component

**Files:**
- Create: `website/src/components/landing/QuickStart.astro`

- [ ] **Step 1: Create QuickStart.astro**

Create `website/src/components/landing/QuickStart.astro`:

```astro
---
---

<section class="qs-section">
  <div class="kk-container">
    <div class="qs-tag">[7] 快速开始</div>
    <h2>三步开跑</h2>
    <div class="term">
      <div class="line"><span class="c">#</span> <span class="d">一条命令装好</span></div>
      <div class="line cmd">brew install tw93/tap/kaku</div>
      <div class="line"><span class="c">#</span> <span class="d">或者直接下 DMG</span></div>
      <div class="line cmd"><a href="/Kaku/download">kaku.site/download</a></div>
      <div class="line"><span class="c">#</span> <span class="d">打开即用</span></div>
      <div class="line cmd">kaku</div>
    </div>
    <p>装好之后 Kaku 会自动配置你的 shell 环境，无需任何额外步骤。</p>
  </div>
</section>

<style>
  .qs-section {
    padding: var(--kk-space-16) 0;
    border-bottom: 1px dashed var(--kk-border);
  }
  .qs-tag {
    color: var(--kk-accent);
    font-family: var(--kk-font-mono);
    font-size: 11px;
    letter-spacing: 1px;
    opacity: 0.7;
    margin-bottom: var(--kk-space-3);
    text-align: center;
  }
  h2 {
    color: var(--kk-text);
    font-size: 24px;
    text-align: center;
    margin: 0 0 var(--kk-space-8);
  }
  .term {
    background: #000;
    border: 1px solid var(--kk-border-strong);
    border-radius: var(--kk-radius-md);
    padding: var(--kk-space-6);
    font-family: var(--kk-font-mono);
    font-size: 13px;
    max-width: 520px;
    margin: 0 auto var(--kk-space-4);
  }
  .line { margin-bottom: var(--kk-space-2); }
  .line:last-child { margin-bottom: 0; }
  .c { color: var(--kk-text-muted); }
  .d { color: var(--kk-text-muted); }
  .cmd { color: var(--kk-accent); }
  .cmd a { color: var(--kk-accent); text-decoration: underline; }
  p {
    color: var(--kk-text-dim);
    font-size: 13px;
    text-align: center;
    max-width: 520px;
    margin: 0 auto;
  }
</style>
```

- [ ] **Step 2: Build**

Run: `cd website && pnpm build`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add website/src/components/landing/QuickStart.astro
git commit -m "feat(website): add quick start section"
```

---

### Task 2.8: WhyKaku component (author story)

**Files:**
- Create: `website/src/components/landing/WhyKaku.astro`

- [ ] **Step 1: Create WhyKaku.astro**

Create `website/src/components/landing/WhyKaku.astro`:

```astro
---
---

<section class="wk-section">
  <div class="kk-container">
    <div class="wk-tag">[8] Why Kaku</div>
    <div class="card">
      <div class="avatar">
        <img src="https://github.com/tw93.png" alt="Tw93" />
      </div>
      <div class="body">
        <h2>为什么我做了 Kaku</h2>
        <p>
          我重度依赖终端工作，此前做过
          <a href="https://github.com/tw93/Pake">Pake</a> 和
          <a href="https://github.com/tw93/mole">Mole</a> —— 这些工具都是为了让日常开发更顺手。
          Kaku 是我对"AI 编码时代的终端"的答案：把 WezTerm 的渲染性能和 Lua 扩展力保留下来，
          默认更好看，并把 AI 助手做进来。希望你也喜欢。
        </p>
        <p class="signoff">—— Tw93 · <a href="https://twitter.com/HiTw93">@HiTw93</a></p>
      </div>
    </div>
  </div>
</section>

<style>
  .wk-section {
    padding: var(--kk-space-16) 0;
    border-bottom: 1px dashed var(--kk-border);
  }
  .wk-tag {
    color: var(--kk-accent);
    font-family: var(--kk-font-mono);
    font-size: 11px;
    letter-spacing: 1px;
    opacity: 0.7;
    margin-bottom: var(--kk-space-6);
    text-align: center;
  }
  .card {
    display: flex;
    gap: var(--kk-space-6);
    align-items: flex-start;
    background: var(--kk-bg-card);
    border: 1px solid var(--kk-border);
    border-radius: var(--kk-radius-md);
    padding: var(--kk-space-6);
    max-width: 640px;
    margin: 0 auto;
  }
  .avatar img {
    width: 64px;
    height: 64px;
    border-radius: 50%;
    border: 1px solid var(--kk-accent);
  }
  .body { flex: 1; }
  .body h2 {
    color: var(--kk-text);
    font-size: 18px;
    margin: 0 0 var(--kk-space-3);
  }
  .body p {
    color: var(--kk-text-dim);
    font-size: 14px;
    line-height: 1.6;
    margin: 0 0 var(--kk-space-3);
  }
  .body p:last-child { margin-bottom: 0; }
  .body a { color: var(--kk-accent); }
  .signoff { font-family: var(--kk-font-mono); font-size: 13px; }
  @media (max-width: 600px) {
    .card { flex-direction: column; align-items: center; text-align: center; }
  }
</style>
```

- [ ] **Step 2: Build**

Run: `cd website && pnpm build`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add website/src/components/landing/WhyKaku.astro
git commit -m "feat(website): add why kaku section"
```

---

### Task 2.9: FAQ component

**Files:**
- Create: `website/src/components/landing/FAQ.astro`

- [ ] **Step 1: Create FAQ.astro**

Create `website/src/components/landing/FAQ.astro`:

```astro
---
interface Item {
  q: string;
  a: string;
}
const items: Item[] = [
  { q: 'Kaku 和 iTerm2 有什么区别？', a: 'iTerm2 是 macOS 原生、Objective-C 写的，功能最全但仅限 macOS。Kaku 基于 Rust + GPU 渲染，跨平台，默认就有 AI 助手、Lua 配置、Lazygit / Yazi 集成。' },
  { q: 'Kaku 和 Warp 有什么区别？', a: 'Warp 闭源、需要登录、带云端功能。Kaku 开源（MIT）、零账号、数据在本地。' },
  { q: 'Kaku 收费吗？', a: '不。Kaku 完全开源免费，MIT 协议。' },
  { q: 'Kaku 开源吗？', a: '是。源码在 GitHub: github.com/tw93/Kaku。欢迎 PR 和 Issue。' },
  { q: 'Kaku 支持 Windows 吗？', a: 'v1 首发 macOS，WezTerm 上游支持 Linux 和 Windows，Kaku 后续会跟进。' },
  { q: '用 Kaku AI 功能会上传我的代码吗？', a: '只有你主动触发 AI 建议时，当次对话内容才会发给你配置的 LLM Provider。Kaku 本身不收集任何数据、不接任何 telemetry。' },
];
---

<section class="faq-section">
  <div class="kk-container">
    <div class="faq-tag">[9] FAQ</div>
    <h2>常见问题</h2>
    <div class="list">
      {items.map(item => (
        <details>
          <summary>{item.q}</summary>
          <p>{item.a}</p>
        </details>
      ))}
    </div>
    <div class="final-cta">
      <a class="cta" href="/Kaku/download">↓ 下载 Kaku</a>
    </div>
  </div>
</section>

<style>
  .faq-section {
    padding: var(--kk-space-16) 0;
  }
  .faq-tag {
    color: var(--kk-accent);
    font-family: var(--kk-font-mono);
    font-size: 11px;
    letter-spacing: 1px;
    opacity: 0.7;
    margin-bottom: var(--kk-space-3);
    text-align: center;
  }
  h2 {
    color: var(--kk-text);
    font-size: 24px;
    text-align: center;
    margin: 0 0 var(--kk-space-8);
  }
  .list { max-width: 640px; margin: 0 auto; }
  details {
    background: var(--kk-bg-card);
    border: 1px solid var(--kk-border);
    border-radius: var(--kk-radius-md);
    padding: var(--kk-space-4);
    margin-bottom: var(--kk-space-3);
  }
  details[open] { border-color: var(--kk-border-strong); }
  summary {
    color: var(--kk-text);
    font-size: 14px;
    cursor: pointer;
    list-style: none;
    font-weight: 500;
  }
  summary::before { content: '▸ '; color: var(--kk-accent); }
  details[open] summary::before { content: '▾ '; }
  details p {
    color: var(--kk-text-dim);
    font-size: 13px;
    line-height: 1.6;
    margin: var(--kk-space-3) 0 0;
  }
  .final-cta {
    text-align: center;
    margin-top: var(--kk-space-12);
  }
  .cta {
    display: inline-block;
    background: var(--kk-accent);
    color: var(--kk-bg);
    padding: var(--kk-space-3) var(--kk-space-6);
    border-radius: var(--kk-radius-sm);
    font-family: var(--kk-font-mono);
    font-size: 14px;
    font-weight: 600;
  }
  .cta:hover { background: var(--kk-accent-dim); text-decoration: none; }
</style>
```

- [ ] **Step 2: Build**

Run: `cd website && pnpm build`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add website/src/components/landing/FAQ.astro
git commit -m "feat(website): add faq section with final cta"
```

---

### Task 2.10: Assemble zh landing page

**Files:**
- Create: `website/src/pages/index.astro`

- [ ] **Step 1: Create index.astro**

Create `website/src/pages/index.astro`:

```astro
---
import LandingLayout from '../layouts/LandingLayout.astro';
import Hero from '../components/landing/Hero.astro';
import TerminalDemo from '../components/landing/TerminalDemo.astro';
import FeatureGrid from '../components/landing/FeatureGrid.astro';
import AIShowcase from '../components/landing/AIShowcase.astro';
import MigrateTabs from '../components/landing/MigrateTabs.astro';
import ScreenshotGallery from '../components/landing/ScreenshotGallery.astro';
import QuickStart from '../components/landing/QuickStart.astro';
import WhyKaku from '../components/landing/WhyKaku.astro';
import FAQ from '../components/landing/FAQ.astro';
---

<LandingLayout
  title="Kaku — 为 AI 编码而生的终端"
  description="Kaku 是基于 WezTerm 深度定制的终端，内建 AI 助手、零配置开箱即用。"
  lang="zh-CN"
>
  <Hero
    title="A fast terminal for AI coding."
    subtitle="为 AI 编码而生的终端 · WezTerm 深度定制 · 零配置开箱即用"
    downloadLabel="下载 DMG"
    brewCommand="brew install tw93/tap/kaku"
    stars="2.3k"
  />
  <TerminalDemo />
  <FeatureGrid />
  <AIShowcase />
  <MigrateTabs />
  <ScreenshotGallery />
  <QuickStart />
  <WhyKaku />
  <FAQ />
</LandingLayout>
```

- [ ] **Step 2: Build and visual-review**

Run: `cd website && pnpm build && pnpm preview`

Open http://localhost:4321/Kaku in a browser. Walk through all 9 sections top-to-bottom. Check:
- Hero CTA buttons are clickable
- Terminal demo animation plays and loops
- Feature grid has 6 cells
- AI showcase renders
- Migrate tabs switch between iTerm2 and WezTerm content on click
- Screenshots show 6 images (all same placeholder is fine)
- Quick start code block is readable
- Why Kaku card shows Tw93 avatar
- FAQ items expand/collapse
- Footer shows 4 columns

If anything is visually broken, fix inline before committing. Stop the preview server with Ctrl+C.

- [ ] **Step 3: Commit**

```bash
git add website/src/pages/index.astro
git commit -m "feat(website): assemble zh landing page"
```

---

### Task 2.11: Playwright smoke test for landing page

**Files:**
- Create: `website/playwright.config.ts`
- Create: `website/tests/smoke.spec.ts`

- [ ] **Step 1: Install Playwright browsers**

Run: `cd website && pnpm exec playwright install chromium`
Expected: downloads Chromium.

- [ ] **Step 2: Create playwright.config.ts**

Create `website/playwright.config.ts`:

```typescript
import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: './tests',
  webServer: {
    command: 'pnpm build && pnpm preview --port 4321',
    url: 'http://localhost:4321/Kaku/',
    reuseExistingServer: !process.env.CI,
    timeout: 120_000,
  },
  use: {
    baseURL: 'http://localhost:4321',
  },
});
```

- [ ] **Step 3: Create smoke test**

Create `website/tests/smoke.spec.ts`:

```typescript
import { test, expect } from '@playwright/test';

test('landing page renders all 9 sections with primary CTA', async ({ page }) => {
  await page.goto('/Kaku/');

  await expect(page.locator('h1')).toContainText('AI coding');

  await expect(page.getByRole('link', { name: /下载 DMG/ })).toBeVisible();

  const sectionTags = [
    '[2] 错误自动修复',
    '[3] 6 核心特性',
    '[4] 自然语言 → 命令',
    '[5] 迁移指南',
    '[6] 主题与截图',
    '[7] 快速开始',
    '[8] Why Kaku',
    '[9] FAQ',
  ];
  for (const tag of sectionTags) {
    await expect(page.getByText(tag, { exact: true })).toBeVisible();
  }

  await expect(page.getByRole('contentinfo')).toContainText('Built on');
  await expect(page.getByRole('contentinfo')).toContainText('MIT License');
});

test('migrate tabs switch content', async ({ page }) => {
  await page.goto('/Kaku/');
  const iTermPanel = page.locator('[data-panel="iterm2"]');
  const wezPanel = page.locator('[data-panel="wezterm"]');
  await expect(iTermPanel).toHaveClass(/active/);
  await page.getByRole('tab', { name: '我是 WezTerm 用户' }).click();
  await expect(wezPanel).toHaveClass(/active/);
  await expect(iTermPanel).not.toHaveClass(/active/);
});
```

- [ ] **Step 4: Run test**

Run: `cd website && pnpm test:smoke`
Expected: 2 tests PASS.

If any assertion fails, fix the underlying component, re-run, don't weaken the assertion.

- [ ] **Step 5: Wrap footer in a semantic <footer> with role="contentinfo"**

Verify `website/src/components/SiteFooter.astro` uses `<footer>` — it already does. Playwright `getByRole('contentinfo')` matches `<footer>` elements that are not inside `<article>` / `<section>`, which our layout satisfies. If the test fails on this assertion, check nesting.

- [ ] **Step 6: Commit**

```bash
git add website/playwright.config.ts website/tests/smoke.spec.ts
git commit -m "test(website): add playwright smoke tests for landing page"
```

---

## Phase 3 — Secondary Pages

### Task 3.1: /download page

**Files:**
- Create: `website/src/pages/download.astro`

- [ ] **Step 1: Create download.astro**

Create `website/src/pages/download.astro`:

```astro
---
import LandingLayout from '../layouts/LandingLayout.astro';
---

<LandingLayout title="下载 Kaku" description="下载 Kaku 终端 · macOS DMG · Homebrew · 源码编译" lang="zh-CN">
  <section class="dl">
    <div class="kk-container">
      <div class="tag">~/kaku $ download</div>
      <h1>下载 Kaku</h1>
      <p class="sub">选一种你喜欢的方式</p>

      <div class="methods">
        <a class="method" href="https://github.com/tw93/Kaku/releases/latest">
          <div class="icon">📦</div>
          <strong>macOS DMG</strong>
          <span>最新 release · 经过 Apple 公证 · 直接拖到应用程序</span>
          <span class="btn">前往 GitHub Releases →</span>
        </a>

        <div class="method">
          <div class="icon">🍺</div>
          <strong>Homebrew</strong>
          <span>一条命令装好，支持自动更新</span>
          <pre><code>brew install tw93/tap/kaku</code></pre>
        </div>

        <a class="method" href="https://github.com/tw93/Kaku#building-from-source">
          <div class="icon">⚙️</div>
          <strong>从源码编译</strong>
          <span>需要 Rust 工具链 · 适合 contributor</span>
          <span class="btn">编译指南 →</span>
        </a>
      </div>
    </div>
  </section>
</LandingLayout>

<style>
  .dl { padding: var(--kk-space-16) 0; }
  .tag {
    color: var(--kk-accent);
    font-family: var(--kk-font-mono);
    font-size: 13px;
    opacity: 0.7;
    margin-bottom: var(--kk-space-4);
    text-align: center;
  }
  h1 {
    color: var(--kk-text);
    font-size: 40px;
    text-align: center;
    margin: 0 0 var(--kk-space-3);
  }
  .sub {
    color: var(--kk-text-dim);
    text-align: center;
    margin: 0 0 var(--kk-space-12);
  }
  .methods {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: var(--kk-space-4);
    max-width: 960px;
    margin: 0 auto;
  }
  .method {
    background: var(--kk-bg-card);
    border: 1px solid var(--kk-border);
    border-radius: var(--kk-radius-md);
    padding: var(--kk-space-6);
    display: flex;
    flex-direction: column;
    gap: var(--kk-space-3);
    text-decoration: none;
    color: inherit;
  }
  a.method:hover { border-color: var(--kk-accent); }
  .icon { font-size: 28px; }
  .method strong { color: var(--kk-text); font-size: 18px; }
  .method span { color: var(--kk-text-dim); font-size: 13px; }
  .method pre {
    background: #000;
    border: 1px solid var(--kk-border);
    border-radius: var(--kk-radius-sm);
    padding: var(--kk-space-3);
    font-size: 12px;
    margin: 0;
    color: var(--kk-accent);
    overflow-x: auto;
  }
  .btn { color: var(--kk-accent) !important; font-family: var(--kk-font-mono); margin-top: auto; }
  @media (max-width: 720px) {
    .methods { grid-template-columns: 1fr; }
  }
</style>
```

- [ ] **Step 2: Build**

Run: `cd website && pnpm build`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add website/src/pages/download.astro
git commit -m "feat(website): add download page"
```

---

### Task 3.2: /roadmap page

**Files:**
- Create: `website/src/content-pages/roadmap-data.ts`
- Create: `website/src/pages/roadmap.astro`

- [ ] **Step 1: Create roadmap-data.ts**

Create `website/src/content-pages/roadmap-data.ts`:

```typescript
export interface RoadmapItem {
  title: string;
  desc?: string;
  issue?: string;
}

export interface RoadmapStage {
  label: string;
  desc: string;
  items: RoadmapItem[];
}

export const roadmap: RoadmapStage[] = [
  {
    label: 'Now',
    desc: '本迭代正在做',
    items: [
      { title: '官方网站 v1 上线', desc: 'Astro + Starlight 落地页和文档站' },
      { title: '错误自动修复体验优化', desc: '更精准的命令匹配和 diff 预览' },
    ],
  },
  {
    label: 'Next',
    desc: '下迭代规划',
    items: [
      { title: '浅色主题网站皮肤', desc: '补齐 light mode' },
      { title: '/showcase 开放社区截图投稿' },
      { title: '更多 AI Provider 预设（Kimi / 豆包 / DeepSeek）' },
      { title: 'Linux 包（deb / rpm）' },
    ],
  },
  {
    label: 'Later',
    desc: '想法已在，待排期',
    items: [
      { title: 'Windows 支持', desc: '跟进 WezTerm 上游' },
      { title: '会话录制与回放' },
      { title: '内建 tmux 协议支持' },
      { title: 'VSCode / JetBrains IDE 集成' },
    ],
  },
];
```

- [ ] **Step 2: Create roadmap.astro**

Create `website/src/pages/roadmap.astro`:

```astro
---
import LandingLayout from '../layouts/LandingLayout.astro';
import { roadmap } from '../content-pages/roadmap-data';
---

<LandingLayout title="Roadmap · Kaku" description="Kaku 的路线图与开发计划" lang="zh-CN">
  <section class="rm">
    <div class="kk-container">
      <div class="tag">~/kaku $ roadmap</div>
      <h1>Roadmap</h1>
      <p class="sub">Kaku 正在做什么、接下来做什么、未来想做什么</p>

      {roadmap.map(stage => (
        <div class="stage">
          <h2>{stage.label}</h2>
          <p class="stage-desc">{stage.desc}</p>
          <ul>
            {stage.items.map(item => (
              <li>
                <strong>{item.title}</strong>
                {item.desc && <span class="desc"> — {item.desc}</span>}
                {item.issue && <a href={item.issue} class="issue">#issue</a>}
              </li>
            ))}
          </ul>
        </div>
      ))}

      <p class="note">
        想提议新功能？欢迎在
        <a href="https://github.com/tw93/Kaku/issues">GitHub Issues</a>
        发起讨论。
      </p>
    </div>
  </section>
</LandingLayout>

<style>
  .rm { padding: var(--kk-space-16) 0; }
  .tag {
    color: var(--kk-accent);
    font-family: var(--kk-font-mono);
    font-size: 13px;
    opacity: 0.7;
    margin-bottom: var(--kk-space-4);
    text-align: center;
  }
  h1 {
    color: var(--kk-text);
    font-size: 40px;
    text-align: center;
    margin: 0 0 var(--kk-space-3);
  }
  .sub {
    color: var(--kk-text-dim);
    text-align: center;
    margin: 0 0 var(--kk-space-12);
  }
  .stage {
    background: var(--kk-bg-card);
    border: 1px solid var(--kk-border);
    border-radius: var(--kk-radius-md);
    padding: var(--kk-space-6);
    margin-bottom: var(--kk-space-6);
  }
  .stage h2 {
    color: var(--kk-accent);
    font-family: var(--kk-font-mono);
    font-size: 18px;
    margin: 0 0 var(--kk-space-2);
    letter-spacing: 1px;
  }
  .stage-desc {
    color: var(--kk-text-muted);
    font-size: 12px;
    margin: 0 0 var(--kk-space-4);
  }
  .stage ul { margin: 0; padding-left: var(--kk-space-6); }
  .stage li {
    color: var(--kk-text);
    font-size: 14px;
    margin-bottom: var(--kk-space-3);
  }
  .stage li .desc { color: var(--kk-text-dim); font-size: 13px; }
  .stage li .issue {
    color: var(--kk-accent);
    font-family: var(--kk-font-mono);
    font-size: 11px;
    margin-left: var(--kk-space-2);
  }
  .note {
    color: var(--kk-text-dim);
    text-align: center;
    margin-top: var(--kk-space-12);
    font-size: 13px;
  }
</style>
```

- [ ] **Step 3: Build**

Run: `cd website && pnpm build`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add website/src/content-pages/ website/src/pages/roadmap.astro
git commit -m "feat(website): add roadmap page"
```

---

### Task 3.3: /changelog page

**Files:**
- Create: `website/src/pages/changelog.astro`

**Design note:** v1 uses static content synced manually from GitHub Releases. A later task can fetch the Releases API at build time. For the first release we keep it simple.

- [ ] **Step 1: Create changelog.astro**

Create `website/src/pages/changelog.astro`:

```astro
---
import LandingLayout from '../layouts/LandingLayout.astro';

interface Release {
  version: string;
  date: string;
  highlights: string[];
  url: string;
}

// Manual sync for v1. Replace with build-time fetch in a later task.
const releases: Release[] = [
  {
    version: 'v0.5.0',
    date: '2026-04-01',
    highlights: [
      '新增 AI Tools 面板，支持 Claude Code / Codex / Gemini CLI 快速切换',
      '修复全屏模式下关闭窗口导致崩溃的问题',
      '双击标签栏即可重命名',
    ],
    url: 'https://github.com/tw93/Kaku/releases/tag/v0.5.0',
  },
];
---

<LandingLayout title="Changelog · Kaku" description="Kaku 更新日志" lang="zh-CN">
  <section class="cl">
    <div class="kk-container">
      <div class="tag">~/kaku $ changelog</div>
      <h1>Changelog</h1>
      <p class="sub">完整的版本历史见 <a href="https://github.com/tw93/Kaku/releases">GitHub Releases</a></p>

      {releases.map(rel => (
        <article class="release">
          <header>
            <h2>{rel.version}</h2>
            <time>{rel.date}</time>
          </header>
          <ul>
            {rel.highlights.map(h => <li>{h}</li>)}
          </ul>
          <a class="more" href={rel.url}>完整 release notes →</a>
        </article>
      ))}
    </div>
  </section>
</LandingLayout>

<style>
  .cl { padding: var(--kk-space-16) 0; }
  .tag {
    color: var(--kk-accent);
    font-family: var(--kk-font-mono);
    font-size: 13px;
    opacity: 0.7;
    margin-bottom: var(--kk-space-4);
    text-align: center;
  }
  h1 {
    color: var(--kk-text);
    font-size: 40px;
    text-align: center;
    margin: 0 0 var(--kk-space-3);
  }
  .sub {
    color: var(--kk-text-dim);
    text-align: center;
    margin: 0 0 var(--kk-space-12);
  }
  .release {
    background: var(--kk-bg-card);
    border: 1px solid var(--kk-border);
    border-radius: var(--kk-radius-md);
    padding: var(--kk-space-6);
    margin-bottom: var(--kk-space-6);
  }
  .release header {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    margin-bottom: var(--kk-space-4);
  }
  .release h2 {
    color: var(--kk-accent);
    font-family: var(--kk-font-mono);
    margin: 0;
  }
  .release time {
    color: var(--kk-text-muted);
    font-family: var(--kk-font-mono);
    font-size: 12px;
  }
  .release ul { margin: 0; padding-left: var(--kk-space-6); }
  .release li {
    color: var(--kk-text);
    font-size: 14px;
    margin-bottom: var(--kk-space-2);
  }
  .more {
    display: inline-block;
    margin-top: var(--kk-space-4);
    color: var(--kk-accent);
    font-family: var(--kk-font-mono);
    font-size: 12px;
  }
</style>
```

- [ ] **Step 2: Build**

Run: `cd website && pnpm build`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add website/src/pages/changelog.astro
git commit -m "feat(website): add changelog page"
```

---

## Phase 4 — Docs Import

### Task 4.1: Create docs landing (task-oriented cards)

**Files:**
- Modify: `website/src/content/docs/index.mdx`

- [ ] **Step 1: Rewrite docs/index.mdx as task-oriented landing**

Replace `website/src/content/docs/index.mdx` content with:

```mdx
---
title: Kaku 文档
description: 为 AI 编码而生的终端
template: splash
hero:
  tagline: 为 AI 编码而生的终端
  actions:
    - text: 快速开始
      link: /docs/start/install/
      icon: right-arrow
      variant: primary
    - text: GitHub
      link: https://github.com/tw93/Kaku
      icon: external
---

import { CardGrid, LinkCard } from '@astrojs/starlight/components';

## 根据你的目标选一条路径

<CardGrid>
  <LinkCard
    title="🚀 我是新用户"
    description="5 分钟快速开始，从安装到起飞"
    href="/docs/start/install/"
  />
  <LinkCard
    title="🔧 我要配置"
    description="Lua 配置完全兼容 WezTerm"
    href="/docs/config/lua/"
  />
  <LinkCard
    title="🤖 我想用 AI"
    description="错误修复、自然语言转命令、Provider 配置"
    href="/docs/ai/overview/"
  />
  <LinkCard
    title="⌨️ 我要查快捷键"
    description="完整快捷键速查表"
    href="/docs/config/keybindings/"
  />
  <LinkCard
    title="🔄 我从 iTerm2 迁移"
    description="保留你熟悉的习惯"
    href="/docs/start/migrate-iterm2/"
  />
  <LinkCard
    title="🔄 我从 WezTerm 迁移"
    description="配置零迁移，直接复用"
    href="/docs/start/migrate-wezterm/"
  />
</CardGrid>
```

- [ ] **Step 2: Build**

Run: `cd website && pnpm build`
Expected: PASS. If Starlight complains about missing sidebar slugs, they'll be added in Task 4.2.

- [ ] **Step 3: Commit**

```bash
git add website/src/content/docs/index.mdx
git commit -m "feat(website): add task-oriented docs landing"
```

---

### Task 4.2: Import existing docs and create stubs

**Files:**
- Create: `website/src/content/docs/start/install.md`
- Create: `website/src/content/docs/start/first-config.md`
- Create: `website/src/content/docs/start/migrate-iterm2.md`
- Create: `website/src/content/docs/start/migrate-wezterm.md`
- Create: `website/src/content/docs/ai/overview.md`
- Create: `website/src/content/docs/ai/error-recovery.md`
- Create: `website/src/content/docs/ai/nl-to-command.md`
- Create: `website/src/content/docs/ai/providers.md`
- Create: `website/src/content/docs/config/lua.md`
- Create: `website/src/content/docs/config/theme.md`
- Create: `website/src/content/docs/config/font.md`
- Create: `website/src/content/docs/config/keybindings.md`
- Create: `website/src/content/docs/features/tabs-windows.md`
- Create: `website/src/content/docs/features/panes-broadcast.md`
- Create: `website/src/content/docs/features/integrations.md`
- Create: `website/src/content/docs/features/shell.md`
- Create: `website/src/content/docs/reference/cli.md`
- Create: `website/src/content/docs/reference/faq.md`
- Create: `website/src/content/docs/reference/changelog.md`

**Strategy:** For each destination file, either (a) copy content from an existing `docs/*.md` file and add Starlight frontmatter, or (b) create a stub with a heading and a one-line description. Every file needs the frontmatter block with `title:` and `description:`.

- [ ] **Step 1: Copy cli.md → reference/cli.md**

Read `docs/cli.md`. Prepend Starlight frontmatter:

```
---
title: CLI 命令
description: Kaku 命令行接口完整参考
---
```

Save to `website/src/content/docs/reference/cli.md`.

- [ ] **Step 2: Copy configuration.md → config/lua.md**

Read `docs/configuration.md`. Prepend:

```
---
title: Lua 配置
description: Kaku 的 Lua 配置方式，完全兼容 WezTerm
---
```

Save to `website/src/content/docs/config/lua.md`.

- [ ] **Step 3: Copy keybindings.md → config/keybindings.md**

Read `docs/keybindings.md`. Prepend:

```
---
title: 快捷键
description: Kaku 完整快捷键速查
---
```

Save to `website/src/content/docs/config/keybindings.md`.

- [ ] **Step 4: Copy faq.md → reference/faq.md**

Read `docs/faq.md`. Prepend:

```
---
title: FAQ
description: Kaku 常见问题解答
---
```

Save to `website/src/content/docs/reference/faq.md`.

- [ ] **Step 5: Copy features.md → ai/overview.md**

Read `docs/features.md`. Prepend:

```
---
title: AI 功能总览
description: Kaku 内建 AI 助手的完整介绍
---
```

Save to `website/src/content/docs/ai/overview.md`.

- [ ] **Step 6: Create all remaining stubs**

For each of the following 15 files, write the content shown. They are intentionally stub pages whose content will be filled in later.

`website/src/content/docs/start/install.md`:
```md
---
title: 安装
description: 通过 DMG 或 Homebrew 安装 Kaku
---

## 通过 Homebrew

\`\`\`sh
brew install tw93/tap/kaku
\`\`\`

## 通过 DMG

访问 [GitHub Releases](https://github.com/tw93/Kaku/releases/latest) 下载最新版本的 DMG 文件，拖到应用程序文件夹即可。

## 首次启动

Kaku 会自动配置你的 shell 环境，无需额外步骤。
```

`website/src/content/docs/start/first-config.md`:
```md
---
title: 首次配置
description: Kaku 开箱即用，但你可以通过 Lua 深度定制
---

## 零配置是默认

Kaku 不需要任何配置就能用：默认字体、默认主题、默认快捷键都是精心调过的。

## 什么时候需要配置

当你想自定义快捷键、颜色、字体或 AI Provider 时，参考 [Lua 配置](/docs/config/lua/)。
```

`website/src/content/docs/start/migrate-iterm2.md`:
```md
---
title: 从 iTerm2 迁移
description: iTerm2 用户切换到 Kaku 的完整指南
---

## 熟悉的快捷键

Kaku 保留了 iTerm2 最常用的快捷键：⌘T 新标签、⌘W 关闭、⌘D 分屏。

## 配置迁移

iTerm2 的 plist 配置无法直接迁移，但 Kaku 的默认配置已经足够开箱即用。
```

`website/src/content/docs/start/migrate-wezterm.md`:
```md
---
title: 从 WezTerm 迁移
description: WezTerm 用户切换到 Kaku 的完整指南
---

## 配置零迁移

你现有的 \`~/.wezterm.lua\` 或 \`~/.config/wezterm/wezterm.lua\` 可以直接被 Kaku 复用。

## 差异点

Kaku 增加了 AI 相关的配置项和更好的默认值。详见 [Lua 配置](/docs/config/lua/)。
```

`website/src/content/docs/ai/error-recovery.md`:
```md
---
title: 错误自动修复
description: 命令失败时，Kaku 自动建议修复方案
---

## 工作原理

当 shell 命令以非零退出码结束时，Kaku 把错误输出发给你配置的 LLM，让它建议一个修复命令。按 ⌘⇧E 一键应用。
```

`website/src/content/docs/ai/nl-to-command.md`:
```md
---
title: 自然语言转命令
description: 用中文描述需求，Kaku 生成对应的 shell 命令
---

## 使用方式

在 shell 提示符前打 \`#\` 加自然语言描述，回车触发。Kaku 调用 LLM 生成命令并注入回提示符，你可以审查后按回车执行。
```

`website/src/content/docs/ai/providers.md`:
```md
---
title: AI Provider 配置
description: 配置 OpenAI、Claude、Gemini 等 LLM Provider
---

## 内建 Provider 预设

打开 \`kaku ai\` 可以看到 Provider 下拉菜单。选一个预设会自动填好 Base URL。

## 自定义 Provider

选 "Custom" 手动填写任何兼容 OpenAI 协议的 Base URL 和 API Key。
```

`website/src/content/docs/config/theme.md`:
```md
---
title: 主题与配色
description: Kaku 的主题切换与自定义配色
---

## 跟随系统

Kaku 默认会跟随 macOS 的系统明暗模式自动切换。

## 手动切换

通过 Lua 配置强制某个主题，参考 [Lua 配置](/docs/config/lua/)。
```

`website/src/content/docs/config/font.md`:
```md
---
title: 字体与渲染
description: 字体配置和字符渲染
---

## 默认字体

Kaku 默认使用 JetBrains Mono，针对 macOS 做了字体渲染调优。

## 自定义字体

通过 \`config.font\` 指定任意等宽字体，支持 fallback 字体链。
```

`website/src/content/docs/features/tabs-windows.md`:
```md
---
title: 标签与窗口
description: 标签栏、窗口管理
---

## 常用快捷键

- ⌘T 新标签
- ⌘W 关闭
- ⌘1-9 快速切换
- ⌘⇧[/] 切换相邻标签
```

`website/src/content/docs/features/panes-broadcast.md`:
```md
---
title: 分屏与广播输入
description: 分屏布局和多 pane 广播输入
---

## 分屏

- ⌘D 垂直分屏
- ⌘⇧D 水平分屏
- ⌘⌥ + 方向键切换

## 广播输入

把同一条命令同时发到多个 pane。参考 [快捷键](/docs/config/keybindings/)。
```

`website/src/content/docs/features/integrations.md`:
```md
---
title: Lazygit / Yazi 集成
description: Kaku 内建 Lazygit 和 Yazi 快捷启动
---

## Lazygit

⌘⇧G 一键打开 Lazygit。

## Yazi

⌘⇧Y 或在 shell 里输入 \`y\` 打开 Yazi 文件管理器。
```

`website/src/content/docs/features/shell.md`:
```md
---
title: Shell 集成
description: Kaku 自动配置的 Shell 插件与工具
---

## 默认 Shell 插件

Kaku 首次启动时会自动配置你的 shell，加入常用 zsh 插件。
```

`website/src/content/docs/reference/changelog.md`:
```md
---
title: 更新日志
description: Kaku 的版本历史
---

完整的版本历史见 [/changelog](/changelog) 或 [GitHub Releases](https://github.com/tw93/Kaku/releases)。
```

- [ ] **Step 7: Build**

Run: `cd website && pnpm build`
Expected: PASS. No broken sidebar slugs.

- [ ] **Step 8: Commit**

```bash
git add website/src/content/docs/
git commit -m "feat(website): import existing docs and create structural stubs"
```

---

### Task 4.3: Create minimal English mirror

**Files:**
- Create: `website/src/content/docs/en/index.mdx`
- Create: `website/src/pages/en/index.astro`

**Strategy:** v1 English support is a "soft" bilingual — the structure exists, the landing and docs index are translated, but deeper English docs auto-fallback or show a "Translation in progress" banner. Full English translation is a content task post-launch.

- [ ] **Step 1: Create English docs landing**

Create `website/src/content/docs/en/index.mdx`:

```mdx
---
title: Kaku Docs
description: A fast terminal for AI coding
template: splash
hero:
  tagline: A fast terminal for AI coding
  actions:
    - text: Get Started
      link: /en/docs/start/install/
      icon: right-arrow
      variant: primary
    - text: GitHub
      link: https://github.com/tw93/Kaku
      icon: external
---

:::caution[Translation in progress]
The English documentation is a work in progress. For the most complete content, see the [Chinese docs](/docs/).
:::

import { CardGrid, LinkCard } from '@astrojs/starlight/components';

<CardGrid>
  <LinkCard title="🚀 New to Kaku" description="5-minute quickstart" href="/en/docs/start/install/" />
  <LinkCard title="🤖 Use AI" description="Error recovery and NL-to-command" href="/en/docs/ai/overview/" />
</CardGrid>
```

- [ ] **Step 2: Create English landing page**

Create `website/src/pages/en/index.astro`:

```astro
---
import LandingLayout from '../../layouts/LandingLayout.astro';
import Hero from '../../components/landing/Hero.astro';
import TerminalDemo from '../../components/landing/TerminalDemo.astro';
import FeatureGrid from '../../components/landing/FeatureGrid.astro';
import AIShowcase from '../../components/landing/AIShowcase.astro';
import MigrateTabs from '../../components/landing/MigrateTabs.astro';
import ScreenshotGallery from '../../components/landing/ScreenshotGallery.astro';
import QuickStart from '../../components/landing/QuickStart.astro';
import WhyKaku from '../../components/landing/WhyKaku.astro';
import FAQ from '../../components/landing/FAQ.astro';
---

<LandingLayout
  title="Kaku — A fast terminal for AI coding"
  description="Kaku is a deeply customized fork of WezTerm with a built-in AI assistant and zero-config defaults."
  lang="en"
>
  <Hero
    title="A fast terminal for AI coding."
    subtitle="A deeply customized WezTerm fork · Zero config · AI built in"
    downloadLabel="Download DMG"
    brewCommand="brew install tw93/tap/kaku"
    stars="2.3k"
  />
  <TerminalDemo />
  <FeatureGrid />
  <AIShowcase />
  <MigrateTabs />
  <ScreenshotGallery />
  <QuickStart />
  <WhyKaku />
  <FAQ />
</LandingLayout>
```

Note: For v1 the components use Chinese copy embedded in their source. English content parity is deferred. The landing page is bilingual in structure, Chinese in content. This is acceptable per the spec's "中文为主、英文可切换" policy and the risk table ("v1 只中文完整、英文机翻 + 关键页人工").

- [ ] **Step 3: Build**

Run: `cd website && pnpm build`
Expected: PASS. `/en/` routes exist in `dist/`.

- [ ] **Step 4: Commit**

```bash
git add website/src/content/docs/en/ website/src/pages/en/
git commit -m "feat(website): add english locale structure (soft bilingual)"
```

---

## Phase 5 — Deployment

### Task 5.1: GitHub Actions workflow for Pages

**Files:**
- Create: `.github/workflows/deploy-website.yml`

- [ ] **Step 1: Create deploy workflow**

Create `.github/workflows/deploy-website.yml`:

```yaml
name: Deploy Website

on:
  push:
    branches: [main, web-ui]
    paths:
      - 'website/**'
      - '.github/workflows/deploy-website.yml'
  workflow_dispatch:

permissions:
  contents: read
  pages: write
  id-token: write

concurrency:
  group: pages
  cancel-in-progress: false

jobs:
  build:
    runs-on: ubuntu-latest
    defaults:
      run:
        working-directory: website
    steps:
      - uses: actions/checkout@v4
      - uses: pnpm/action-setup@v4
        with:
          version: 9
      - uses: actions/setup-node@v4
        with:
          node-version: 20
          cache: pnpm
          cache-dependency-path: website/pnpm-lock.yaml
      - run: pnpm install --frozen-lockfile
      - run: pnpm build
      - uses: actions/upload-pages-artifact@v3
        with:
          path: website/dist

  deploy:
    needs: build
    runs-on: ubuntu-latest
    environment:
      name: github-pages
      url: ${{ steps.deployment.outputs.page_url }}
    steps:
      - id: deployment
        uses: actions/deploy-pages@v4
```

- [ ] **Step 2: Enable GitHub Pages in repo settings (manual)**

**Manual step for the engineer:** Go to https://github.com/tw93/Kaku/settings/pages and set Source to "GitHub Actions". This cannot be done via CLI/commit.

Leave a note in the task: the first deployment will fail until this is toggled.

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/deploy-website.yml
git commit -m "ci(website): add github pages deploy workflow"
```

- [ ] **Step 4: Push branch and watch Actions**

Run: `git push -u origin web-ui`

Then visit https://github.com/tw93/Kaku/actions to watch the `Deploy Website` workflow. Expected: build job passes, deploy job deploys to https://tw93.github.io/Kaku/.

If the deploy job fails with "Pages not enabled": toggle Pages source to "GitHub Actions" in repo settings, then re-run the workflow from the Actions tab.

---

### Task 5.2: Final Lighthouse + smoke verification

**Files:**
- None (verification-only task)

- [ ] **Step 1: Run final smoke test locally**

Run: `cd website && pnpm test:smoke`
Expected: both tests PASS.

- [ ] **Step 2: Build production preview and run Lighthouse**

Run: `cd website && pnpm build && pnpm preview --port 4321`

In Chrome, open http://localhost:4321/Kaku/. Open DevTools → Lighthouse tab. Select Mobile + Performance + Accessibility + Best Practices + SEO. Click "Analyze page load".

Expected: all 4 categories ≥ 95.

If any score < 95:
- Performance < 95 → check large images (the reused `kaku.jpg` placeholders may be too large). Resize to max 1920x1200 and re-save as `.webp` if needed.
- Accessibility < 95 → missing `alt`, missing `aria-label`, insufficient contrast. Inspect the failing audits and fix the specific element.
- Best Practices < 95 → usually console errors or deprecated APIs. Check DevTools console.
- SEO < 95 → missing meta description, missing lang attribute. Both should already be in `LandingLayout`.

Fix inline, re-run Lighthouse.

- [ ] **Step 3: Verify live deployment**

In a browser, visit https://tw93.github.io/Kaku/.

Checklist:
- [ ] Home page loads with Terminal Hacker theme
- [ ] Terminal demo animation plays
- [ ] All 9 homepage sections visible
- [ ] `/Kaku/download` works
- [ ] `/Kaku/roadmap` works
- [ ] `/Kaku/changelog` works
- [ ] `/Kaku/docs/` loads docs landing
- [ ] Pagefind search box appears on docs pages
- [ ] Sidebar shows 5 top-level sections
- [ ] Footer renders with 4 columns

If any link 404s, it's most likely a `base: '/Kaku'` path issue. Check the component's `href` and make sure it starts with `/Kaku/`.

- [ ] **Step 4: Commit (verification record)**

No code changes expected at this step. If you had to fix anything in Step 2, commit it as `fix(website): ...` before doing Step 3.

If everything passed first try:

```bash
git commit --allow-empty -m "chore(website): v1 verification passed (lighthouse + live deploy)"
```

---

## Done Criteria

v1 of the Kaku website is considered complete when:

1. `pnpm build` passes in `website/` with no errors or warnings
2. `pnpm test:smoke` passes (2 Playwright tests)
3. Lighthouse mobile scores ≥ 95 on all four categories for the landing page
4. Live deployment at https://tw93.github.io/Kaku/ is reachable
5. All 9 homepage sections render correctly
6. 5 docs sections are navigable from the sidebar
7. `/download`, `/roadmap`, `/changelog` all render
8. Footer appears on every page
9. No console errors on any page

Out of scope for v1 (lands in v2):
- `/blog`, `/showcase`
- English translation parity
- Light theme
- Real screenshots (replacing `kaku.jpg` placeholder)
- `/changelog` auto-sync from GitHub Releases
- `/roadmap` auto-sync from GitHub Projects
- Custom domain `kaku.site`

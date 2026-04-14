/**
 * Kaku website — Astro + Starlight config.
 *
 * Pinned versions:
 * - @astrojs/sitemap is pinned to 3.2.1 via pnpm.overrides in package.json.
 *   Reason: sitemap >= 3.7 calls the `astro:routes:resolved` hook which only
 *   exists in Astro 5.x. We are on Astro 4.x, so 3.7+ crashes at build time.
 *   When upgrading to Astro 5, remove the override.
 */
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

export default defineConfig({
  site: 'https://tw93.github.io',
  base: '/Kaku',
  integrations: [
    starlight({
      title: 'Kaku',
      description: '为 AI 编码而生的终端',
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
            { label: '安装', slug: 'docs/start/install' },
            { label: '首次配置', slug: 'docs/start/first-config' },
            { label: '从 iTerm2 迁移', slug: 'docs/start/migrate-iterm2' },
            { label: '从 WezTerm 迁移', slug: 'docs/start/migrate-wezterm' },
          ],
        },
        {
          label: 'AI 功能',
          items: [
            { label: '总览', slug: 'docs/ai/overview' },
            { label: '错误自动修复', slug: 'docs/ai/error-recovery' },
            { label: '自然语言转命令', slug: 'docs/ai/nl-to-command' },
            { label: 'Provider 配置', slug: 'docs/ai/providers' },
          ],
        },
        {
          label: '配置',
          items: [
            { label: 'Lua 配置', slug: 'docs/config/lua' },
            { label: '主题与配色', slug: 'docs/config/theme' },
            { label: '字体与渲染', slug: 'docs/config/font' },
            { label: '快捷键', slug: 'docs/config/keybindings' },
          ],
        },
        {
          label: '功能详解',
          items: [
            { label: '标签与窗口', slug: 'docs/features/tabs-windows' },
            { label: '分屏与广播输入', slug: 'docs/features/panes-broadcast' },
            { label: 'Lazygit / Yazi 集成', slug: 'docs/features/integrations' },
            { label: 'Shell 集成', slug: 'docs/features/shell' },
          ],
        },
        {
          label: '参考',
          items: [
            { label: 'CLI 命令', slug: 'docs/reference/cli' },
            { label: 'FAQ', slug: 'docs/reference/faq' },
            { label: '更新日志', slug: 'docs/reference/changelog' },
          ],
        },
      ],
    }),
  ],
});

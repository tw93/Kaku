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
          translations: { en: 'Getting Started' },
          items: [
            { label: '安装', translations: { en: 'Install' }, slug: 'docs/start/install' },
            { label: '首次配置', translations: { en: 'First Configuration' }, slug: 'docs/start/first-config' },
            { label: '从 iTerm2 迁移', translations: { en: 'Migrating from iTerm2' }, slug: 'docs/start/migrate-iterm2' },
            { label: '从 WezTerm 迁移', translations: { en: 'Migrating from WezTerm' }, slug: 'docs/start/migrate-wezterm' },
          ],
        },
        {
          label: 'AI 功能',
          translations: { en: 'AI Features' },
          items: [
            { label: '总览', translations: { en: 'Overview' }, slug: 'docs/ai/overview' },
            { label: '错误自动修复', translations: { en: 'Error Recovery' }, slug: 'docs/ai/error-recovery' },
            { label: '自然语言转命令', translations: { en: 'Natural Language to Command' }, slug: 'docs/ai/nl-to-command' },
            { label: 'Provider 配置', translations: { en: 'Providers' }, slug: 'docs/ai/providers' },
          ],
        },
        {
          label: '配置',
          translations: { en: 'Configuration' },
          items: [
            { label: 'Lua 配置', translations: { en: 'Lua Configuration' }, slug: 'docs/config/lua' },
            { label: '主题与配色', translations: { en: 'Theme & Colors' }, slug: 'docs/config/theme' },
            { label: '字体与渲染', translations: { en: 'Fonts & Rendering' }, slug: 'docs/config/font' },
            { label: '快捷键', translations: { en: 'Keybindings' }, slug: 'docs/config/keybindings' },
          ],
        },
        {
          label: '功能详解',
          translations: { en: 'Features' },
          items: [
            { label: '标签与窗口', translations: { en: 'Tabs & Windows' }, slug: 'docs/features/tabs-windows' },
            { label: '分屏与广播输入', translations: { en: 'Panes & Broadcast' }, slug: 'docs/features/panes-broadcast' },
            { label: 'Lazygit / Yazi 集成', translations: { en: 'Lazygit / Yazi' }, slug: 'docs/features/integrations' },
            { label: 'Shell 集成', translations: { en: 'Shell Integration' }, slug: 'docs/features/shell' },
          ],
        },
        {
          label: '参考',
          translations: { en: 'Reference' },
          items: [
            { label: 'CLI 命令', translations: { en: 'CLI Commands' }, slug: 'docs/reference/cli' },
            { label: 'FAQ', translations: { en: 'FAQ' }, slug: 'docs/reference/faq' },
            { label: '更新日志', translations: { en: 'Changelog' }, slug: 'docs/reference/changelog' },
          ],
        },
      ],
    }),
  ],
});

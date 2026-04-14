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
      // sidebar: commented out — slug stubs not yet created (Task 4.2 will restore)
      sidebar: [],
    }),
  ],
});

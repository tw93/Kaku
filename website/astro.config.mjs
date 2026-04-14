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
      // sidebar: commented out — slug stubs not yet created (Task 4.2 will restore)
      sidebar: [],
    }),
  ],
});

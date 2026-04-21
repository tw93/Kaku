import type { RoadmapStage } from './roadmap-data';

export const roadmapEn: RoadmapStage[] = [
  {
    label: 'Now',
    desc: 'Shipping this iteration',
    items: [
      { title: 'Official website v1', desc: 'Astro + Starlight landing page and docs site' },
      { title: 'Error recovery polish', desc: 'Sharper command matching and a diff preview' },
    ],
  },
  {
    label: 'Next',
    desc: 'Planned for the next iteration',
    items: [
      { title: 'Light theme for the website', desc: 'Round out the light mode' },
      { title: '/showcase community screenshot submissions' },
      { title: 'More built-in AI provider presets (Kimi / Doubao / DeepSeek)' },
      { title: 'More macOS distribution and update polish' },
    ],
  },
  {
    label: 'Later',
    desc: 'On the wishlist, not yet scheduled',
    items: [
      { title: 'Non-macOS platform evaluation', desc: 'No timeline; macOS quality comes first' },
      { title: 'Session recording and replay' },
      { title: 'Native tmux protocol support' },
      { title: 'VSCode / JetBrains IDE integrations' },
    ],
  },
];

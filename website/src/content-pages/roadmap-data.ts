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

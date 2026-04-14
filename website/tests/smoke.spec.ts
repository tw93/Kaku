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

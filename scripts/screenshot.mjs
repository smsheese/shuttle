#!/usr/bin/env node
/** Screenshot Shuttle UI at desktop and mobile viewports for gauntlet comparison. */
import { chromium } from 'playwright';
import { fileURLToPath } from 'url';
import path from 'path';
import fs from 'fs';
import { spawn } from 'child_process';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.join(__dirname, '..');
const OUT = path.join(ROOT, 'screenshots', 'ours');
const PREVIEW_PORT = 4173;

async function freePreviewPort() {
  try {
    const { execSync } = await import('child_process');
    execSync(`fuser -k ${PREVIEW_PORT}/tcp 2>/dev/null || true`, { stdio: 'ignore' });
    await new Promise((r) => setTimeout(r, 500));
  } catch {
    // best effort
  }
}

async function startPreview() {
  await freePreviewPort();
  const proc = spawn('npm', ['run', 'preview', '--', '--port', String(PREVIEW_PORT), '--host', '127.0.0.1'], {
    cwd: path.join(ROOT, 'shuttle-app'),
    stdio: ['ignore', 'pipe', 'pipe'],
  });

  const base = `http://127.0.0.1:${PREVIEW_PORT}`;
  const deadline = Date.now() + 30000;
  while (Date.now() < deadline) {
    try {
      const res = await fetch(base);
      if (res.ok) return proc;
    } catch {
      // server not ready yet
    }
    await new Promise((r) => setTimeout(r, 250));
  }

  proc.kill();
  throw new Error('Preview server timeout');
}

const views = [
  { name: 'unified-inbox-desktop', width: 1280, height: 800 },
  { name: 'unified-inbox-mobile', width: 390, height: 844 },
  { name: 'conversation-list-desktop', width: 1280, height: 800, action: 'list-only' },
  { name: 'thread-view-desktop', width: 1280, height: 800, action: 'select-first' },
];

async function main() {
  fs.mkdirSync(OUT, { recursive: true });
  fs.mkdirSync(path.join(ROOT, 'screenshots', 'reference'), { recursive: true });

  const preview = await startPreview();
  await new Promise((r) => setTimeout(r, 1500));

  const browser = await chromium.launch();
  const base = `http://127.0.0.1:${PREVIEW_PORT}`;

  for (const view of views) {
    const page = await browser.newPage({ viewport: { width: view.width, height: view.height } });
    await page.goto(base, { waitUntil: 'networkidle' });
    await page.waitForTimeout(500);

    if (view.action === 'select-first') {
      const conv = page.locator('.conv-item').first();
      if (await conv.count()) {
        await conv.click();
        await page.waitForSelector('.msg-row', { timeout: 5000 });
        const bubbleCount = await page.locator('.bubble').count();
        if (bubbleCount < 5) {
          throw new Error(`Expected rich thread (5+ bubbles), got ${bubbleCount}`);
        }
        await page.waitForTimeout(300);
      }
    }

    await page.screenshot({ path: path.join(OUT, `${view.name}.png`), fullPage: false });
    await page.close();
  }

  // Account setup — clean first-run state (?setup=1 hides inbox, no demo data loaded)
  const setupPage = await browser.newPage({ viewport: { width: 1280, height: 800 } });
  await setupPage.goto(`${base}?setup=1`, { waitUntil: 'networkidle' });
  await setupPage.waitForSelector('.modal h2', { timeout: 5000 });
  await setupPage.waitForSelector('.security-banner', { timeout: 5000 });
  await setupPage.waitForSelector('.add-btn', { timeout: 5000 });
  await setupPage.waitForTimeout(400);
  const modal = setupPage.locator('.modal');
  await modal.screenshot({ path: path.join(OUT, 'account-setup-desktop.png') });
  await setupPage.close();

  await browser.close();
  preview.kill();
  console.log(`Screenshots saved to ${OUT}`);
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});

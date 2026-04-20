// Auto-updater: checks on startup and every 6h, shows banner with changelog.

import { invoke } from './utils.js';

const CHECK_INTERVAL_MS = 6 * 60 * 60 * 1000;

const check = () => window.__TAURI__?.updater?.check();
const relaunch = () => window.__TAURI__?.process?.relaunch();
const listen = (event, cb) => window.__TAURI__?.event?.listen(event, cb);

let pendingUpdate = null;
let checking = false;

function ensureBanner() {
  let banner = document.getElementById('update-banner');
  if (banner) return banner;

  banner = document.createElement('div');
  banner.id = 'update-banner';
  banner.style.cssText = `
    position: fixed; top: 0; left: 0; right: 0; z-index: 9999;
    background: #0b6bcb; color: white; padding: 12px 16px;
    font-size: 13px; display: none; box-shadow: 0 2px 8px rgba(0,0,0,0.2);
  `;
  banner.innerHTML = `
    <div style="max-width: 900px; margin: 0 auto; display: flex; gap: 16px; align-items: flex-start;">
      <div style="flex: 1;">
        <div style="font-weight: 600; margin-bottom: 4px;">
          Update available: <span id="update-version"></span>
        </div>
        <ul id="update-notes" style="margin: 4px 0 0 18px; padding: 0; font-size: 12px; opacity: 0.95;"></ul>
      </div>
      <div style="display: flex; gap: 8px; flex-shrink: 0;">
        <button id="update-install" style="background: white; color: #0b6bcb; border: 0; padding: 6px 12px; border-radius: 4px; font-weight: 600; cursor: pointer;">Install & Restart</button>
        <button id="update-dismiss" style="background: transparent; color: white; border: 1px solid rgba(255,255,255,0.5); padding: 6px 12px; border-radius: 4px; cursor: pointer;">Later</button>
      </div>
    </div>
  `;
  document.body.appendChild(banner);

  banner.querySelector('#update-install').addEventListener('click', installPending);
  banner.querySelector('#update-dismiss').addEventListener('click', () => {
    banner.style.display = 'none';
  });
  return banner;
}

function renderBanner(update) {
  const banner = ensureBanner();
  banner.querySelector('#update-version').textContent = `v${update.version}`;
  const notesEl = banner.querySelector('#update-notes');
  notesEl.innerHTML = '';
  const lines = String(update.body || '')
    .split('\n')
    .map(l => l.replace(/^[-*]\s*/, '').trim())
    .filter(Boolean);
  for (const line of lines) {
    const li = document.createElement('li');
    li.textContent = line;
    notesEl.appendChild(li);
  }
  banner.style.display = 'block';
}

async function installPending() {
  if (!pendingUpdate) return;
  const btn = document.getElementById('update-install');
  if (btn) { btn.disabled = true; btn.textContent = 'Installing...'; }
  try {
    await pendingUpdate.downloadAndInstall();
    await relaunch();
  } catch (err) {
    console.error('[updater] install failed', err);
    if (btn) { btn.disabled = false; btn.textContent = 'Install & Restart'; }
    alert('Update failed: ' + (err?.message || err));
  }
}

async function runCheck({ silent = true } = {}) {
  if (checking) return;
  checking = true;
  try {
    const update = await check();
    if (update) {
      pendingUpdate = update;
      renderBanner(update);
      try { await invoke('set_update_tray_label', { version: update.version }); } catch (_) {}
    } else if (!silent) {
      alert('You are on the latest version.');
    }
  } catch (err) {
    console.error('[updater] check failed', err);
    if (!silent) alert('Update check failed: ' + (err?.message || err));
  } finally {
    checking = false;
  }
}

export function initUpdater() {
  if (!window.__TAURI__?.updater) return;
  runCheck({ silent: true });
  setInterval(() => runCheck({ silent: true }), CHECK_INTERVAL_MS);
  if (listen) {
    listen('tray-update-clicked', () => {
      if (pendingUpdate) {
        ensureBanner().style.display = 'block';
      } else {
        runCheck({ silent: false });
      }
    });
  }
}

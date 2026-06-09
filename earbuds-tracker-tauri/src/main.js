// main.js – Nox frontend logic
// Tauri v2 exposes invoke under window.__TAURI__.core, not window.__TAURI__ directly
const invoke = (window.__TAURI__ && window.__TAURI__.core && window.__TAURI__.core.invoke) || (async (cmd, args) => {
  console.log(`[Mock Invoke] ${cmd}`, args);
  if (cmd === 'get_battery_graph_data') {
    return [
      { label: "Jun 1 10:15", left_start: 90, left_end: 80, right_start: 92, right_end: 82, case_start: 70, case_end: 70, duration_mins: 35 },
      { label: "Jun 2 12:30", left_start: 80, left_end: 65, right_start: 82, right_end: 66, case_start: 70, case_end: 65, duration_mins: 45 },
      { label: "Jun 3 09:00", left_start: 100, left_end: 50, right_start: 100, right_end: 48, case_start: 100, case_end: 85, duration_mins: 120 },
      { label: "Jun 4 18:20", left_start: 50, left_end: 35, right_start: 48, right_end: 33, case_start: 85, case_end: 85, duration_mins: 40 },
      { label: "Jun 5 11:10", left_start: 85, left_end: 70, right_start: 83, right_end: 68, case_start: 85, case_end: 80, duration_mins: 55 }
    ];
  }
  if (cmd === 'get_paired_devices') return ["CMF Buds 2a", "Nothing Ear (2)"];
  if (cmd === 'get_battery_interval') return 10;
  if (cmd === 'get_battery_step') return 5;
  if (cmd === 'get_snapshot') return { connected: true, playback: true };
  if (cmd === 'get_daily_history') {
    const weekOffset = args?.weekOffset || 0;
    const base = new Date();
    base.setHours(0, 0, 0, 0);
    base.setDate(base.getDate() - (weekOffset * 7));
    const rows = [];
    for (let i = 6; i >= 0; i--) {
      const d = new Date(base);
      d.setDate(base.getDate() - i);
      const day = getLocalDateString(d);
      rows.push({
        day,
        connected_secs: Math.max(0, 1800 + (6 - i) * 600 - weekOffset * 120),
        playback_secs: Math.max(0, 1200 + (6 - i) * 480 - weekOffset * 90),
      });
    }
    return rows;
  }
  if (cmd === 'get_daily_history_bounds') {
    const oldest = new Date();
    oldest.setHours(0, 0, 0, 0);
    oldest.setDate(oldest.getDate() - 35);
    return {
      oldest_day: getLocalDateString(oldest),
      newest_day: getLocalDateString(new Date()),
    };
  }
  if (cmd === 'get_query_log') {
    return [
      { session_id: 12, session_start: '2026-06-06T09:45:00', event_ts: '2026-06-06T10:00:00', action: 'Battery query sent', details: 'Polling CMF Buds 2a' },
      { session_id: 12, session_start: '2026-06-06T09:45:00', event_ts: '2026-06-06T10:00:01', action: 'Battery response received', details: 'L=Some(90) R=Some(90) C=Some(100)' },
    ];
  }
  if (cmd === 'export_all_data') {
    return JSON.stringify({
      sessions: [],
      daily_stats: [],
      app_audio_events: [],
      query_logs: [],
    });
  }
  if (cmd === 'import_all_data') return true;
  if (cmd === 'get_app_version') return '1.0.0';
  if (cmd === 'get_auto_backup_settings') return { enabled: false, interval: 'never' };
  if (cmd === 'set_auto_backup_settings') return { enabled: !!args?.enabled, interval: args?.interval || 'never' };
  if (cmd === 'run_auto_backup') return { exported_at: new Date().toISOString(), auto_backup_path: '', download_path: '', sessions: 0, daily_stats: 0, app_audio_events: 0, query_logs: 0 };
  return [];
});
const event = window.__TAURI__ && window.__TAURI__.event;

// ── Preferences State ──────────────────────────────────────────────────────────
let currentGoal = (() => {
  const storedGoal = parseFloat(localStorage.getItem('playback-goal') || '2.0');
  return Number.isFinite(storedGoal) ? Math.min(12, Math.max(0.5, storedGoal)) : 2.0;
})();
let notificationsEnabled = localStorage.getItem('notifications-enabled') !== 'false';
let startupEnabled = localStorage.getItem('startup-enabled') === 'true';
let currentDeviceName = localStorage.getItem('target-device') || 'CMF Buds 2a';
let fontStyle = localStorage.getItem('font-style') || 'default';
let batteryPollIntervalSec = 10;

// Send initial device name to backend
invoke('set_device_name', { name: currentDeviceName }).catch(console.error);

// State trackers for notifications & animations
let wasConnected = null;
let lastSessConn = 0;
let lastSessPlay = 0;
let lastConnected = false;
let lastPlaying = false;
let lastGoalMet = false;
let lastBatteryUpdateAt = Number(localStorage.getItem('last-battery-update-at') || 0);
let pulseFactor = 0;
let animationFrameId = null;
let mousePos = null;

function getLocalDateString(date = new Date()) {
  const y = date.getFullYear();
  const m = String(date.getMonth() + 1).padStart(2, '0');
  const d = String(date.getDate()).padStart(2, '0');
  return `${y}-${m}-${d}`;
}

function getCachedTodayPlaybackSecs() {
  const cacheDate = localStorage.getItem('daily-playback-cache-date');
  if (cacheDate !== getLocalDateString()) return 0;

  const cachedSecs = parseFloat(localStorage.getItem('daily-playback-cache-secs') || '0');
  return Number.isFinite(cachedSecs) ? cachedSecs : 0;
}

function storeTodayPlaybackSecs(secs) {
  if (!Number.isFinite(secs) || secs < 0) return;
  localStorage.setItem('daily-playback-cache-date', getLocalDateString());
  localStorage.setItem('daily-playback-cache-secs', String(secs));
}

// Elements
const goalInput = document.getElementById('goal-input');
const notificationToggle = document.getElementById('notification-toggle');
const startupToggle = document.getElementById('startup-toggle');
const deviceNameInput = document.getElementById('device-name-input');
const fontStyleSelect = document.getElementById('font-style-select');
const batteryStepSelect = document.getElementById('battery-step-select');
const importDataFile = document.getElementById('import-data-file');
const exportDataBtn = document.getElementById('export-data-btn');
const importDataBtn = document.getElementById('import-data-btn');
const importInfoDialog = document.getElementById('import-info-dialog');
const importInfoCancel = document.getElementById('import-info-cancel');
const importInfoConfirm = document.getElementById('import-info-confirm');
const importSuccessDialog = document.getElementById('import-success-dialog');
const importSuccessMsg = document.getElementById('import-success-msg');
const importSuccessClose = document.getElementById('import-success-close');
const exportInfoDialog = document.getElementById('export-info-dialog');
const exportInfoCancel = document.getElementById('export-info-cancel');
const exportInfoConfirm = document.getElementById('export-info-confirm');
const exportSuccessDialog = document.getElementById('export-success-dialog');
const exportSuccessMsg = document.getElementById('export-success-msg');
const exportSuccessClose = document.getElementById('export-success-close');
const resetSuccessDialog = document.getElementById('reset-success-dialog');
const resetSuccessMsg = document.getElementById('reset-success-msg');
const resetSuccessClose = document.getElementById('reset-success-close');
const graphDurationSelect = document.getElementById('graph-duration');
const graphDurationNote = document.getElementById('graph-duration-note');
const queryLogBtn = document.getElementById('query-log-btn');
const queryLogDialog = document.getElementById('query-log-dialog');
const queryLogList = document.getElementById('query-log-list');
const queryLogCount = document.getElementById('query-log-count');
const queryLogClose = document.getElementById('query-log-close');
const queryLogRefresh = document.getElementById('query-log-refresh');

function applyFontStyle(value) {
  const mode = value === 'ndot' ? 'ndot' : 'default';
  fontStyle = mode;
  localStorage.setItem('font-style', mode);
  document.body.classList.toggle('font-ndot', mode === 'ndot');
  if (fontStyleSelect && fontStyleSelect.value !== mode) {
    fontStyleSelect.value = mode;
  }
}

// Initialize settings inputs
if (goalInput) {
  goalInput.value = currentGoal;
  goalInput.addEventListener('change', (e) => {
    const parsed = parseFloat(e.target.value);
    currentGoal = Number.isFinite(parsed) ? Math.min(12, Math.max(0.5, parsed)) : 2.0;
    e.target.value = currentGoal.toFixed(1);
    localStorage.setItem('playback-goal', currentGoal);
    refreshSnapshot();
    updateDailyStatsAndChart();
  });
}
if (notificationToggle) {
  notificationToggle.checked = notificationsEnabled;
  notificationToggle.addEventListener('change', (e) => {
    notificationsEnabled = e.target.checked;
    localStorage.setItem('notifications-enabled', notificationsEnabled);
    if (notificationsEnabled) {
      Notification.requestPermission();
    }
  });
}
if (startupToggle) {
  // Initialise the toggle from the saved preference first, then reconcile
  // with the OS-registered state once the backend answers. This keeps the
  // UI in sync with whichever source of truth is available first.
  startupToggle.checked = startupEnabled;
  try {
    invoke('get_startup_enabled').then((serverEnabled) => {
      startupEnabled = !!serverEnabled;
      startupToggle.checked = startupEnabled;
      localStorage.setItem('startup-enabled', startupEnabled);
    }).catch(console.error);
  } catch (e) {
    console.error('get_startup_enabled failed', e);
  }
  startupToggle.addEventListener('change', async (e) => {
    const desired = !!e.target.checked;
    const prev = startupEnabled;
    startupEnabled = desired;
    localStorage.setItem('startup-enabled', startupEnabled);
    try {
      const ok = await invoke('set_startup_enabled', { enabled: desired });
      if (!ok) {
        // Revert UI if backend rejected the change
        startupEnabled = prev;
        startupToggle.checked = prev;
        localStorage.setItem('startup-enabled', prev);
        alert('Could not update the Windows startup registration. Please try again.');
      }
    } catch (err) {
      console.error('set_startup_enabled failed', err);
      startupEnabled = prev;
      startupToggle.checked = prev;
      localStorage.setItem('startup-enabled', prev);
    }
  });
}

applyFontStyle(fontStyle);
if (fontStyleSelect) {
  fontStyleSelect.value = fontStyle;
  fontStyleSelect.addEventListener('change', (e) => {
    applyFontStyle(e.target.value);
  });
}
async function initDeviceNameDatalist() {
  const input = document.getElementById('device-name-input');
  const datalist = document.getElementById('device-name-list');
  const sub = document.getElementById('dashboard-sub');

  const updateSub = () => {
    if (sub) sub.textContent = `Real-time connection monitoring for ${currentDeviceName}`;
  };
  updateSub();

  if (!input || !datalist) return;

  // Set initial input value
  input.value = currentDeviceName;

  // Fetch paired devices from backend
  let pairedDevices = [];
  try {
    pairedDevices = await invoke('get_paired_devices');
  } catch (e) {
    console.error('get_paired_devices failed', e);
  }

  // Always include CMF Buds 2a in list if not present, as a default fallback
  if (!pairedDevices.includes("CMF Buds 2a")) {
    pairedDevices.unshift("CMF Buds 2a");
  }

  // Populate datalist options
  datalist.innerHTML = '';
  pairedDevices.forEach(dev => {
    const opt = document.createElement('option');
    opt.value = dev;
    datalist.appendChild(opt);
  });

  // Handle input change (either by typing or by selecting from datalist)
  input.addEventListener('change', (e) => {
    const val = e.target.value.trim();
    if (val) {
      currentDeviceName = val;
      localStorage.setItem('target-device', currentDeviceName);
      updateSub();
      invoke('set_device_name', { name: currentDeviceName }).catch(console.error);
    }
  });
}

initDeviceNameDatalist();

async function initBatteryIntervalSelect() {
  const select = document.getElementById('battery-interval-select');
  if (!select) return;

  try {
    const val = await invoke('get_battery_interval');
    batteryPollIntervalSec = Number(val) || 10;
    select.value = String(batteryPollIntervalSec);
  } catch (e) {
    console.error('get_battery_interval failed', e);
  }

  select.addEventListener('change', async (e) => {
    const secs = parseInt(e.target.value);
    if (!isNaN(secs)) {
      batteryPollIntervalSec = secs;
      try {
        await invoke('set_battery_interval', { secs });
      } catch (err) {
        console.error('set_battery_interval failed', err);
      }
    }
  });
}

initBatteryIntervalSelect();

async function initBatteryStepSelect() {
  const select = document.getElementById('battery-step-select');
  if (!select) return;

  try {
    const val = await invoke('get_battery_step');
    select.value = val.toString();
  } catch (e) {
    console.error('get_battery_step failed', e);
  }

  select.addEventListener('change', async (e) => {
    const step = parseInt(e.target.value);
    if (!isNaN(step)) {
      try {
        await invoke('set_battery_step', { step });
      } catch (err) {
        console.error('set_battery_step failed', err);
      }
    }
  });
}

initBatteryStepSelect();

// The duration dropdown IS the toggle: "Never" turns auto backup off, any
// other option turns it on with the matching interval.
async function initAutoBackupSettings() {
  const select = document.getElementById('auto-backup-duration-select');
  if (!select) return;

  let prev = select.value;

  try {
    const settings = await invoke('get_auto_backup_settings');
    if (settings && typeof settings.interval === 'string') {
      select.value = settings.interval;
      prev = settings.interval;
    }
  } catch (e) {
    console.error('get_auto_backup_settings failed', e);
  }

  const persist = async () => {
    const interval = select.value;
    try {
      const updated = await invoke('set_auto_backup_settings', { interval });
      if (updated && typeof updated.interval === 'string') {
        select.value = updated.interval;
      }
    } catch (err) {
      console.error('set_auto_backup_settings failed', err);
      select.value = prev;
    }
  };

  select.addEventListener('change', persist);
}

initAutoBackupSettings();

// Populate the About section's version dynamically from the Tauri config.
async function initAppVersion() {
  const el = document.getElementById('app-version');
  if (!el) return;
  try {
    const version = await invoke('get_app_version');
    if (version) el.textContent = String(version);
  } catch (e) {
    console.error('get_app_version failed', e);
  }
}
initAppVersion();

function closeQueryLogDialog() {
  if (queryLogDialog) {
    queryLogDialog.hidden = true;
  }
}

function closeExportInfoDialog() {
  if (exportInfoDialog) exportInfoDialog.hidden = true;
}

function closeExportSuccessDialog() {
  if (exportSuccessDialog) exportSuccessDialog.hidden = true;
}

function closeImportInfoDialog() {
  if (importInfoDialog) importInfoDialog.hidden = true;
}

function closeImportSuccessDialog() {
  if (importSuccessDialog) importSuccessDialog.hidden = true;
}

function closeResetSuccessDialog() {
  if (resetSuccessDialog) resetSuccessDialog.hidden = true;
}

function escapeHtml(value) {
  return String(value ?? '')
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&#39;');
}

function prettyTimestamp(value) {
  const raw = String(value ?? '').trim();
  if (!raw) return '—';

  const parsed = new Date(raw.replace(' ', 'T'));
  if (!Number.isNaN(parsed.getTime())) {
    return escapeHtml(new Intl.DateTimeFormat('en-US', {
      year: 'numeric',
      month: 'short',
      day: 'numeric',
      hour: 'numeric',
      minute: '2-digit',
      second: '2-digit',
      hour12: true,
    }).format(parsed));
  }

  return escapeHtml(raw.replace('T', ' '));
}

function renderQueryLog(entries) {
  if (!queryLogList) return;

  const list = Array.isArray(entries) ? entries : [];
  if (queryLogCount) {
    queryLogCount.textContent = `${list.length} entr${list.length === 1 ? 'y' : 'ies'}`;
  }

  if (list.length === 0) {
    queryLogList.innerHTML = '<div class="query-log-empty">No earbud queries have been recorded yet.</div>';
    return;
  }

  queryLogList.innerHTML = list.map(entry => `
    <div class="query-log-item">
      <div class="query-log-head">
        <div class="query-log-action">${escapeHtml(entry.action || 'Query')}</div>
        <div class="query-log-time">${prettyTimestamp(entry.event_ts || entry.timestamp || '')}</div>
      </div>
      <div class="query-log-meta">Session #${escapeHtml(entry.session_id ?? '—')} <span class="query-log-session-start">${prettyTimestamp(entry.session_start || '')}</span></div>
      <div class="query-log-details">${escapeHtml(entry.details || '')}</div>
    </div>
  `).join('');
}

async function loadQueryLog() {
  if (!queryLogList) return;
  queryLogList.innerHTML = '<div class="query-log-empty">Loading query log...</div>';
  try {
    const entries = await invoke('get_query_log');
    renderQueryLog(entries);
  } catch (e) {
    console.error('get_query_log failed', e);
    queryLogList.innerHTML = '<div class="query-log-empty">Failed to load the earbud query log.</div>';
    if (queryLogCount) queryLogCount.textContent = '0 entries';
  }
}

async function openQueryLogDialog() {
  if (!queryLogDialog) return;
  queryLogDialog.hidden = false;
  await loadQueryLog();
}

queryLogBtn?.addEventListener('click', () => {
  openQueryLogDialog();
});

queryLogClose?.addEventListener('click', closeQueryLogDialog);
queryLogRefresh?.addEventListener('click', loadQueryLog);

queryLogDialog?.addEventListener('click', (e) => {
  if (e.target === queryLogDialog) {
    closeQueryLogDialog();
  }
});

function buildSettingsBackup() {
  return {
    playback_goal: currentGoal,
    notifications_enabled: notificationsEnabled,
    startup_enabled: startupEnabled,
    target_device: currentDeviceName,
    font_style: fontStyle,
    battery_interval_secs: batteryPollIntervalSec,
    battery_step: parseInt(batteryStepSelect?.value || '5', 10) || 5,
  };
}


async function exportAllData() {
  const exportBtn = exportDataBtn;
  if (exportBtn) exportBtn.disabled = true;
  try {
    closeExportInfoDialog();
    const result = await invoke('export_all_data');
    const details = [
      `Exported at: ${result.exported_at || '—'}`,
      `Sessions: ${result.sessions ?? 0}`,
      `Daily history rows: ${result.daily_stats ?? 0}`,
      `App audio events: ${result.app_audio_events ?? 0}`,
      `Query logs: ${result.query_logs ?? 0}`,
      `Saved to: ${result.export_path || '—'}`,
      `Download copy: ${result.download_path || '—'}`,
    ];
    if (exportSuccessMsg) {
      exportSuccessMsg.innerHTML = details.map(line => `<div>${escapeHtml(line)}</div>`).join('');
    }
    if (exportSuccessDialog) exportSuccessDialog.hidden = false;
  } catch (e) {
    console.error('export_all_data failed', e);
    alert('Export failed. Please try again.');
  } finally {
    if (exportBtn) exportBtn.disabled = false;
  }
}

async function restoreSettingsFromBackup(settings = {}) {
  if (typeof settings.playback_goal === 'number') {
    currentGoal = Math.min(12, Math.max(0.5, settings.playback_goal));
    if (goalInput) goalInput.value = currentGoal.toFixed(1);
    localStorage.setItem('playback-goal', currentGoal);
  }

  if (typeof settings.notifications_enabled === 'boolean') {
    notificationsEnabled = settings.notifications_enabled;
    if (notificationToggle) notificationToggle.checked = notificationsEnabled;
    localStorage.setItem('notifications-enabled', notificationsEnabled);
  }

  if (typeof settings.target_device === 'string' && settings.target_device.trim()) {
    currentDeviceName = settings.target_device.trim();
    localStorage.setItem('target-device', currentDeviceName);
    const deviceSelect = document.getElementById('device-name-select');
    if (deviceSelect && deviceSelect.value !== currentDeviceName && [...deviceSelect.options].some(opt => opt.value === currentDeviceName)) {
      deviceSelect.value = currentDeviceName;
    }
    if (deviceNameInput) deviceNameInput.value = currentDeviceName;
    const dashboardSub = document.getElementById('dashboard-sub');
    if (dashboardSub) dashboardSub.textContent = `Real-time connection monitoring for ${currentDeviceName}`;
    await invoke('set_device_name', { name: currentDeviceName }).catch(console.error);
  }

  if (typeof settings.font_style === 'string' && settings.font_style.trim()) {
    applyFontStyle(settings.font_style.trim());
  }

  if (typeof settings.battery_interval_secs === 'number') {
    batteryPollIntervalSec = settings.battery_interval_secs;
    localStorage.setItem('battery-interval-secs', String(batteryPollIntervalSec));
    if (batteryStepSelect && batteryStepSelect.value) {
      // keep current UI consistent; actual interval select is managed below
    }
    const intervalSelect = document.getElementById('battery-interval-select');
    if (intervalSelect) intervalSelect.value = String(batteryPollIntervalSec);
    await invoke('set_battery_interval', { secs: batteryPollIntervalSec }).catch(console.error);
  }

  if (typeof settings.battery_step === 'number') {
    const step = settings.battery_step;
    if (batteryStepSelect) batteryStepSelect.value = String(step);
    await invoke('set_battery_step', { step }).catch(console.error);
  }

  if (typeof settings.startup_enabled === 'boolean') {
    startupEnabled = settings.startup_enabled;
    if (startupToggle) startupToggle.checked = startupEnabled;
    localStorage.setItem('startup-enabled', startupEnabled);
    await invoke('set_startup_enabled', { enabled: startupEnabled }).catch(console.error);
  }

  await refreshSnapshot();

  updateDailyStatsAndChart();
}

async function importAllDataFromFile(file) {
  if (!file) return;

  try {
    const text = await file.text();
    const payload = JSON.parse(text);
    const database = payload.database || payload;
    const imported = await invoke('import_all_data', { data: JSON.stringify(database) });
    if (!imported) {
      throw new Error('Import failed');
    }
    await restoreSettingsFromBackup(payload.settings || {});
    const dbCounts = {
      sessions: Array.isArray(database.sessions) ? database.sessions.length : 0,
      daily_stats: Array.isArray(database.daily_stats) ? database.daily_stats.length : 0,
      app_audio_events: Array.isArray(database.app_audio_events) ? database.app_audio_events.length : 0,
      query_logs: Array.isArray(database.query_logs) ? database.query_logs.length : 0,
    };
    if (importSuccessMsg) {
      importSuccessMsg.innerHTML = [
        `Imported sessions: ${dbCounts.sessions}`,
        `Imported daily history rows: ${dbCounts.daily_stats}`,
        `Imported app audio events: ${dbCounts.app_audio_events}`,
        `Imported query logs: ${dbCounts.query_logs}`,
        `Settings restored: ${payload.settings ? 'Yes' : 'No'}`,
      ].map(line => `<div>${escapeHtml(line)}</div>`).join('');
    }
    if (importSuccessDialog) importSuccessDialog.hidden = false;
  } catch (e) {
    console.error('import_all_data failed', e);
    alert('Import failed. Please make sure the file is a valid Nox backup.');
  } finally {
    if (importDataFile) importDataFile.value = '';
  }
}

exportDataBtn?.addEventListener('click', () => {
  if (exportInfoDialog) exportInfoDialog.hidden = false;
});
exportInfoCancel?.addEventListener('click', closeExportInfoDialog);
exportInfoConfirm?.addEventListener('click', exportAllData);
exportSuccessClose?.addEventListener('click', closeExportSuccessDialog);
resetSuccessClose?.addEventListener('click', closeResetSuccessDialog);
exportInfoDialog?.addEventListener('click', (e) => {
  if (e.target === exportInfoDialog) closeExportInfoDialog();
});
exportSuccessDialog?.addEventListener('click', (e) => {
  if (e.target === exportSuccessDialog) closeExportSuccessDialog();
});
resetSuccessDialog?.addEventListener('click', (e) => {
  if (e.target === resetSuccessDialog) closeResetSuccessDialog();
});
importInfoCancel?.addEventListener('click', closeImportInfoDialog);
importInfoConfirm?.addEventListener('click', () => {
  closeImportInfoDialog();
  importDataFile?.click();
});
importSuccessClose?.addEventListener('click', closeImportSuccessDialog);
importInfoDialog?.addEventListener('click', (e) => {
  if (e.target === importInfoDialog) closeImportInfoDialog();
});
importSuccessDialog?.addEventListener('click', (e) => {
  if (e.target === importSuccessDialog) closeImportSuccessDialog();
});
importDataBtn?.addEventListener('click', () => {
  if (importInfoDialog) importInfoDialog.hidden = false;
});
importDataFile?.addEventListener('change', () => {
  const file = importDataFile.files?.[0];
  if (file) importAllDataFromFile(file);
});

// Check build mode (debug vs release) to show/hide debug tag
async function checkBuildMode() {
  try {
    const debugMode = await invoke('is_debug');
    const tag = document.getElementById('debug-tag');
    if (tag) {
      tag.style.display = debugMode ? 'inline-block' : 'none';
    }
  } catch (e) {
    console.error('Failed to check build mode', e);
  }
}
checkBuildMode();

// Request permission early if enabled
if (notificationsEnabled && Notification.permission === 'default') {
  Notification.requestPermission();
}

// ── Navigation ────────────────────────────────────────────────────────────────
function navigateToPage(page) {
  const navItem = document.querySelector(`.nav-item[data-page="${page}"]`);
  if (navItem) {
    document.querySelectorAll('.nav-item').forEach(n => n.classList.remove('active'));
    document.querySelectorAll('.page').forEach(p => p.classList.remove('active'));
    navItem.classList.add('active');
    document.getElementById(`page-${page}`).classList.add('active');
    if (page === 'history') loadHistory();
    if (page === 'statistics') updateDailyStatsAndChart();
    if (page === 'battery') loadBatteryGraph();
  }
}

document.querySelectorAll('.nav-item').forEach(item => {
  item.addEventListener('click', () => {
    navigateToPage(item.dataset.page);
  });
});

// Click handlers for cards on the dashboard to allow seamless navigation
const connCard = document.getElementById('conn-time')?.closest('.time-card');
if (connCard) {
  connCard.addEventListener('click', () => navigateToPage('history'));
}
const playCard = document.getElementById('play-time')?.closest('.time-card');
if (playCard) {
  playCard.addEventListener('click', () => navigateToPage('statistics'));
}

// ── Helpers ───────────────────────────────────────────────────────────────────
function fmtFull(secs) {
  const s = Math.max(0, Math.floor(secs));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sc = s % 60;
  return `${String(h).padStart(2, '0')}:${String(m).padStart(2, '0')}:${String(sc).padStart(2, '0')}`;
}

function fmtH(secs) {
  const s = Math.max(0, Math.floor(secs));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sc = s % 60;
  if (h) return `${h}h ${String(m).padStart(2, '0')}m`;
  if (m) return `${m}m ${String(sc).padStart(2, '0')}s`;
  return `${sc}s`;
}

let statsChartState = {
  history: [],
  hoverIndex: -1,
  canvas: null,
  wrapper: null,
  tooltip: null,
  bound: false,
  layout: null,
};
let statsWeekOffset = 0;
let statsMaxWeekOffset = null;
let statsHistoryBoundsPromise = null;

function fmtStatsDate(dateStr) {
  if (!dateStr) return 'Unknown date';
  const parsed = new Date(`${dateStr}T00:00:00`);
  if (Number.isNaN(parsed.getTime())) return dateStr;
  return new Intl.DateTimeFormat('en-US', {
    weekday: 'short',
    month: 'short',
    day: 'numeric',
    year: 'numeric',
  }).format(parsed);
}

function parseGraphTimestamp(value) {
  const raw = String(value ?? '').trim();
  if (!raw) return null;

  const parsed = new Date(raw.replace(' ', 'T'));
  return Number.isNaN(parsed.getTime()) ? null : parsed;
}

function formatGraphBucketLabel(duration, date) {
  if (duration === 'day') {
    return new Intl.DateTimeFormat('en-US', {
      hour: 'numeric',
      minute: '2-digit',
      hour12: true,
    }).format(date);
  }

  return new Intl.DateTimeFormat('en-US', {
    month: 'short',
    day: 'numeric',
  }).format(date);
}

function fmtStatsDayTitle(day) {
  if (!day) return '';
  const parsed = new Date(`${day.dateStr}T00:00:00`);
  if (Number.isNaN(parsed.getTime())) return day.label;
  return new Intl.DateTimeFormat('en-US', {
    weekday: 'short',
    month: 'short',
    day: 'numeric',
  }).format(parsed);
}

function updateGraphDurationState(isConnected) {
  if (graphDurationSelect) {
    const sessionOption = graphDurationSelect.querySelector('option[value="session"]');
    if (sessionOption) {
      sessionOption.textContent = isConnected ? 'This Session' : 'Prev Session';
    }
  }
  if (graphDurationNote) {
    graphDurationNote.textContent = isConnected
      ? 'Live session data overrides all graphs while connected.'
      : 'Shows the previous completed session when disconnected.';
  }
}

function buildRadarTooltipHtml(meta = {}) {
  const rows = [
    ['Category', meta.label || '—'],
    ['Selected value', `${Number(meta.value ?? 0).toFixed(1)}%`],
    ['Source rows', String(meta.rowCount ?? 0)],
    ['Left total', `${Number(meta.leftTotal ?? 0).toFixed(1)}%`],
    ['Right total', `${Number(meta.rightTotal ?? 0).toFixed(1)}%`],
    ['Case total', `${Number(meta.caseTotal ?? 0).toFixed(1)}%`],
    ['Average total', `${Number(meta.avgTotal ?? 0).toFixed(1)}%`],
    ['Max total', `${Number(meta.maxTotal ?? 0).toFixed(1)}%`],
  ];

  return `
    <div class="chart-tooltip">
      <div class="tooltip-title">Radar Details</div>
      <div class="tooltip-row"><span class="tooltip-label">Duration</span><span class="tooltip-value">${escapeHtml(meta.durationLabel || '—')}</span></div>
      <div class="tooltip-row"><span class="tooltip-label">Item</span><span class="tooltip-value">${escapeHtml(meta.itemLabel || '—')}</span></div>
      ${rows.map(([label, value]) => `
        <div class="tooltip-row">
          <span class="tooltip-label">${escapeHtml(label)}</span>
          <span class="tooltip-value">${escapeHtml(value)}</span>
        </div>
      `).join('')}
    </div>
  `;
}

function buildRadarBucketTooltipHtml(meta = {}) {
  const rows = [
    ['Bucket', meta.bucketLabel || '—'],
    ['Series', meta.seriesLabel || '—'],
    ['Value', `${Number(meta.value ?? 0).toFixed(1)}%`],
    ['Source rows', String(meta.rowCount ?? 0)],
    ['Left Bud Drain', `${Number(meta.left ?? 0).toFixed(1)}%`],
    ['Right Bud Drain', `${Number(meta.right ?? 0).toFixed(1)}%`],
    ['Case Drain', `${Number(meta.case ?? 0).toFixed(1)}%`],
  ];

  return `
    <div class="chart-tooltip">
      <div class="tooltip-title">Radar Details</div>
      <div class="tooltip-row"><span class="tooltip-label">Duration</span><span class="tooltip-value">${escapeHtml(meta.durationLabel || '—')}</span></div>
      <div class="tooltip-row"><span class="tooltip-label">Item</span><span class="tooltip-value">${escapeHtml(meta.itemLabel || '—')}</span></div>
      ${rows.map(([label, value]) => `
        <div class="tooltip-row">
          <span class="tooltip-label">${escapeHtml(label)}</span>
          <span class="tooltip-value">${escapeHtml(value)}</span>
        </div>
      `).join('')}
    </div>
  `;
}


function getStatsWeekWindow(weekOffset = 0) {
  const end = new Date();
  end.setHours(0, 0, 0, 0);
  end.setDate(end.getDate() - (weekOffset * 7));
  const start = new Date(end);
  start.setDate(start.getDate() - 6);
  return { start, end };
}

function formatStatsWeekRange(weekOffset = 0) {
  const { start, end } = getStatsWeekWindow(weekOffset);
  const startFmt = new Intl.DateTimeFormat('en-US', {
    month: 'short',
    day: 'numeric',
  }).format(start);
  const endFmt = new Intl.DateTimeFormat('en-US', {
    month: 'short',
    day: 'numeric',
    year: 'numeric',
  }).format(end);
  return `${startFmt} - ${endFmt}`;
}

function calcStatsMaxWeekOffset(bounds) {
  if (!bounds?.oldest_day) return 0;
  const oldest = new Date(`${bounds.oldest_day}T00:00:00`);
  if (Number.isNaN(oldest.getTime())) return 0;
  const today = new Date();
  today.setHours(0, 0, 0, 0);
  const diffDays = Math.max(0, Math.floor((today.getTime() - oldest.getTime()) / 86400000));
  return Math.max(0, Math.floor(diffDays / 7));
}

async function ensureStatsHistoryBounds() {
  if (statsMaxWeekOffset !== null) return statsMaxWeekOffset;
  if (!statsHistoryBoundsPromise) {
    statsHistoryBoundsPromise = invoke('get_daily_history_bounds')
      .then((bounds) => {
        statsMaxWeekOffset = calcStatsMaxWeekOffset(bounds);
        return statsMaxWeekOffset;
      })
      .catch((err) => {
        console.error('get_daily_history_bounds failed', err);
        statsMaxWeekOffset = 0;
        return statsMaxWeekOffset;
      })
      .finally(() => {
        statsHistoryBoundsPromise = null;
      });
  }
  return statsHistoryBoundsPromise;
}

function updateStatsWeekControls() {
  const rangeEl = document.getElementById('stats-week-range');
  const prevBtn = document.getElementById('stats-week-prev');
  const nextBtn = document.getElementById('stats-week-next');
  const canGoOlder = statsMaxWeekOffset === null ? true : statsWeekOffset < statsMaxWeekOffset;
  if (rangeEl) rangeEl.textContent = formatStatsWeekRange(statsWeekOffset);
  if (prevBtn) prevBtn.disabled = !canGoOlder;
  if (nextBtn) nextBtn.disabled = statsWeekOffset <= 0;
}

async function changeStatsWeek(delta) {
  if (delta > 0 && statsMaxWeekOffset !== null && statsWeekOffset >= statsMaxWeekOffset) {
    updateStatsWeekControls();
    return;
  }
  statsWeekOffset = Math.max(0, statsWeekOffset + delta);
  if (statsMaxWeekOffset !== null) {
    statsWeekOffset = Math.min(statsWeekOffset, statsMaxWeekOffset);
  }
  statsChartState.hoverIndex = -1;
  if (statsChartState.tooltip) {
    statsChartState.tooltip.hidden = true;
  }
  await updateDailyStatsAndChart();
}

function buildStatsDays(weekOffset = 0) {
  const { start } = getStatsWeekWindow(weekOffset);
  const days = [];
  for (let i = 0; i < 7; i++) {
    const d = new Date(start);
    d.setDate(start.getDate() + i);
    days.push({
      dateStr: getLocalDateString(d),
      label: new Intl.DateTimeFormat('en-US', {
        weekday: 'short',
        day: 'numeric',
      }).format(d),
      connected: 0,
      playback: 0,
    });
  }
  return days;
}

function bindStatsChartHover() {
  const canvas = document.getElementById('stats-chart');
  const wrapper = canvas?.parentElement;
  const tooltip = document.getElementById('stats-chart-tooltip');
  if (!canvas || !wrapper || !tooltip) return;

  statsChartState.canvas = canvas;
  statsChartState.wrapper = wrapper;
  statsChartState.tooltip = tooltip;

  if (statsChartState.bound) return;
  statsChartState.bound = true;

  const hideTooltip = () => {
    statsChartState.hoverIndex = -1;
    tooltip.hidden = true;
    if (statsChartState.history.length) {
      drawDailyChart(statsChartState.history);
    }
  };

  const updateHover = (clientX, clientY) => {
    const layout = statsChartState.layout;
    if (!layout || !layout.hitBoxes || !layout.hitBoxes.length) return;

    const rect = canvas.getBoundingClientRect();
    const x = clientX - rect.left;
    const y = clientY - rect.top;
    const hitIndex = layout.hitBoxes.findIndex((box) =>
      x >= box.x0 && x <= box.x1 && y >= box.y0 && y <= box.y1
    );

    if (hitIndex === statsChartState.hoverIndex) {
      if (hitIndex >= 0) {
        positionStatsTooltip(layout.days[hitIndex], clientX, clientY);
      }
      return;
    }

    statsChartState.hoverIndex = hitIndex;
    tooltip.hidden = hitIndex < 0;
    if (hitIndex >= 0) {
      positionStatsTooltip(layout.days[hitIndex], clientX, clientY);
    }
    if (statsChartState.history.length) {
      drawDailyChart(statsChartState.history);
    }
  };

  canvas.addEventListener('mousemove', (e) => updateHover(e.clientX, e.clientY));
  canvas.addEventListener('mouseleave', hideTooltip);
  canvas.addEventListener('mouseout', hideTooltip);
}

function positionStatsTooltip(day, clientX, clientY) {
  const tooltip = statsChartState.tooltip;
  const wrapper = statsChartState.wrapper;
  if (!tooltip || !wrapper || !day) return;

  const goalSecs = currentGoal * 3600;
  const goalMet = day.playback >= goalSecs;
  tooltip.innerHTML = `
    <div class="tooltip-title">${fmtStatsDayTitle(day)}</div>
    <div class="tooltip-row"><span class="tooltip-label">Date</span><span class="tooltip-value">${fmtStatsDate(day.dateStr)}</span></div>
    <div class="tooltip-row"><span class="tooltip-label">Connection</span><span class="tooltip-value">${fmtH(day.connected)}</span></div>
    <div class="tooltip-row"><span class="tooltip-label">Playback</span><span class="tooltip-value green">${fmtH(day.playback)}</span></div>
    <div class="tooltip-row"><span class="tooltip-label">Goal</span><span class="tooltip-value">${currentGoal.toFixed(1)}h</span></div>
    <div class="tooltip-row"><span class="tooltip-label">Status</span><span class="tooltip-value ${goalMet ? 'green' : 'amber'}">${goalMet ? 'Goal met' : 'Goal not met'}</span></div>
  `;
  tooltip.hidden = false;

  const wrapperRect = wrapper.getBoundingClientRect();
  const tooltipRect = tooltip.getBoundingClientRect();
  const padding = 12;

  let left = clientX - wrapperRect.left + 14;
  let top = clientY - wrapperRect.top - tooltipRect.height - 14;

  if (left + tooltipRect.width > wrapperRect.width - padding) {
    left = Math.max(padding, wrapperRect.width - tooltipRect.width - padding);
  }
  if (top < padding) {
    top = clientY - wrapperRect.top + 14;
  }
  if (top + tooltipRect.height > wrapperRect.height - padding) {
    top = Math.max(padding, wrapperRect.height - tooltipRect.height - padding);
  }

  tooltip.style.left = `${Math.max(padding, left)}px`;
  tooltip.style.top = `${Math.max(padding, top)}px`;
}

// ── Ring canvas ───────────────────────────────────────────────────────────────
const canvas = document.getElementById('ring-canvas');
const ctx = canvas.getContext('2d');
const dpr = window.devicePixelRatio || 1;
const W = 220;
const H = 220;

canvas.width = W * dpr;
canvas.height = H * dpr;
canvas.style.width = W + 'px';
canvas.style.height = H + 'px';
ctx.scale(dpr, dpr);

// Interactive mouse interactions on the circular ring canvas
if (canvas) {
  const HIT = 20; // hit zone radius around each ring (must match drawRing)

  canvas.addEventListener('mousemove', (e) => {
    const rect = canvas.getBoundingClientRect();
    const mx = e.clientX - rect.left;
    const my = e.clientY - rect.top;
    mousePos = { x: mx, y: my };

    const cx = W / 2, cy = H / 2;
    const rOut = W / 2 - 20;
    const rIn = rOut - 18;
    const dx = mx - cx;
    const dy = my - cy;
    const dist = Math.sqrt(dx * dx + dy * dy);

    if (Math.abs(dist - rOut) < HIT || Math.abs(dist - rIn) < HIT) {
      canvas.style.cursor = 'pointer';
    } else {
      canvas.style.cursor = 'default';
    }
  });

  canvas.addEventListener('mouseleave', () => {
    mousePos = null;
    canvas.style.cursor = 'default';
  });

  canvas.addEventListener('click', (e) => {
    const rect = canvas.getBoundingClientRect();
    const mx = e.clientX - rect.left;
    const my = e.clientY - rect.top;

    const cx = W / 2, cy = H / 2;
    const rOut = W / 2 - 20;
    const rIn = rOut - 18;
    const dx = mx - cx;
    const dy = my - cy;
    const dist = Math.sqrt(dx * dx + dy * dy);

    if (Math.abs(dist - rOut) < HIT) {
      navigateToPage('history');
    } else if (Math.abs(dist - rIn) < HIT) {
      navigateToPage('statistics');
    }
  });
}

const MAX_SECS = 24 * 3600;

function drawRing(connSecs, playSecs, connected, playing, goalMet) {
  ctx.clearRect(0, 0, W, H);
  const cx = W / 2, cy = H / 2;
  const rOut = W / 2 - 20;
  const rIn = rOut - 18;
  const sw = 10;

  const trackOut = playing ? '#2d2d2d' : '#2a2a2e';
  const trackIn = playing ? '#1f3a22' : '#2e2e34';
  const connCol = connected ? '#ffffff' : '#3a3a40';
  const playCol = playing
    ? (goalMet ? '#fbbf24' : '#4ade80')
    : '#2a3a2a';

  // Apply a lighter pulse glow to playback ring when active
  if (playing) {
    ctx.shadowBlur = (goalMet ? 3 : 4) * pulseFactor;
    ctx.shadowColor = goalMet ? '#fbbf24' : '#4ade80';
  } else {
    ctx.shadowBlur = 0;
  }

  function arc(r, start, span, color) {
    ctx.beginPath();
    ctx.arc(cx, cy, r, start, start + span, span < 0);
    ctx.strokeStyle = color;
    ctx.lineWidth = sw;
    ctx.lineCap = 'round';
    ctx.stroke();
  }

  // Tracks
  arc(rOut, 0, Math.PI * 2, trackOut);

  const ROUND_SECS = 3600;

  // Outer progress arc (connection)
  const connSpan = connSecs > 0 ? ((connSecs % ROUND_SECS) / ROUND_SECS) * Math.PI * 2 : 0;
  if (connSpan > 0) {
    arc(rOut, -Math.PI / 2, connSpan, connCol);
  }

  // Inner track
  ctx.shadowBlur = 0; // reset shadow for inner track
  arc(rIn, 0, Math.PI * 2, trackIn);

  // Inner progress arc (playback) with glow
  if (playing) {
    ctx.shadowBlur = (goalMet ? 3 : 4) * pulseFactor;
    ctx.shadowColor = goalMet ? '#fbbf24' : '#4ade80';
  }
  const playSpan = playSecs > 0 ? ((playSecs % ROUND_SECS) / ROUND_SECS) * Math.PI * 2 : 0;
  if (playSpan > 0) {
    arc(rIn, -Math.PI / 2, playSpan, playCol);
  }
  ctx.shadowBlur = 0; // reset



  // ── Center text/visualizer logic ──
  let hoverType = 'none';
  if (mousePos) {
    const dx = mousePos.x - cx;
    const dy = mousePos.y - cy;
    const dist = Math.sqrt(dx * dx + dy * dy);
    if (Math.abs(dist - rOut) < 20) {
      hoverType = 'conn';
    } else if (Math.abs(dist - rIn) < 20) {
      hoverType = 'play';
    }
  }

  ctx.textAlign = 'center';
  ctx.textBaseline = 'middle';

  if (hoverType === 'conn') {
    // Draw a subtle highlight glow on the outer ring
    ctx.shadowBlur = 6;
    ctx.shadowColor = 'rgba(255,255,255,0.2)';
    const span = connSecs > 0 ? (connSpan > 0 ? connSpan : Math.PI * 2) : Math.PI * 2;
    arc(rOut, -Math.PI / 2, span, connSecs > 0 ? '#ffffff' : '#555560');
    ctx.shadowBlur = 0;

    ctx.font = 'bold 10px "Segoe UI", sans-serif';
    ctx.fillStyle = '#888898';
    ctx.fillText('CONNECTION', cx, cy - 14);

    ctx.font = 'bold 14px "Cascadia Code", monospace';
    ctx.fillStyle = '#ffffff';
    const connTimeStr = document.getElementById('conn-time').textContent;
    ctx.fillText(connTimeStr, cx, cy + 8);
  } else if (hoverType === 'play') {
    // Draw a subtle highlight glow on the inner ring
    ctx.shadowBlur = goalMet ? 3 : 6;
    ctx.shadowColor = goalMet ? 'rgba(251,191,36,0.14)' : 'rgba(74,222,128,0.25)';
    const span = playSecs > 0 ? (playSpan > 0 ? playSpan : Math.PI * 2) : Math.PI * 2;
    arc(rIn, -Math.PI / 2, span, playSecs > 0 ? (goalMet ? '#fbbf24' : '#4ade80') : '#334433');
    ctx.shadowBlur = 0;

    ctx.font = 'bold 10px "Segoe UI", sans-serif';
    ctx.fillStyle = '#888898';
    ctx.fillText('PLAYBACK', cx, cy - 14);

    ctx.font = 'bold 14px "Cascadia Code", monospace';
    ctx.fillStyle = goalMet ? '#fbbf24' : '#4ade80';
    const playTimeStr = document.getElementById('play-time').textContent;
    ctx.fillText(playTimeStr, cx, cy + 8);
  } else {
    if (playing) {
      // ── Animated Equalizer Bars (Playing) ──────────────────────────────
      const barColor = goalMet ? '#fbbf24' : '#4ade80';
      const barCount = 5;
      const barW = 5;
      const maxBarH = 22;
      const minBarH = 5;
      const gap = 3;
      const totalW = barCount * barW + (barCount - 1) * gap;
      const startX = cx - totalW / 2;
      const time = Date.now() / 300;

      // Pulse rings behind bars
      const waveCount = 2;
      for (let i = 0; i < waveCount; i++) {
        const offset = (i / waveCount) * Math.PI * 2;
        const scale = 0.5 + Math.sin(time + offset) * 0.5;
        const rad = 10 + (rIn - 20) * scale;
        const opacity = (goalMet ? 0.06 : 0.12) * (1 - scale);
        ctx.beginPath();
        ctx.arc(cx, cy, rad, 0, Math.PI * 2);
        ctx.strokeStyle = goalMet ? `rgba(251,191,36,${opacity})` : `rgba(74,222,128,${opacity})`;
        ctx.lineWidth = 1.5;
        ctx.stroke();
      }

      ctx.fillStyle = barColor;
      for (let i = 0; i < barCount; i++) {
        const phase = (i / barCount) * Math.PI * 2;
        const t = 0.5 + Math.sin(time * 1.3 + phase) * 0.5;
        const barH = minBarH + (maxBarH - minBarH) * t;
        const bx = startX + i * (barW + gap);
        const by = cy - barH / 2;
        ctx.beginPath();
        ctx.roundRect(bx, by, barW, barH, 2);
        ctx.fill();
      }
    } else if (connected) {
      // ── Bluetooth Symbol (Connected/Idle) ──────────────────────────────
      const s = 12; // scale
      const bx = cx, by = cy;
      ctx.strokeStyle = '#fbbf24';
      ctx.lineWidth = 2.2;
      ctx.lineCap = 'round';
      ctx.lineJoin = 'round';
      // Vertical spine
      ctx.beginPath();
      ctx.moveTo(bx, by - s);
      ctx.lineTo(bx, by + s);
      ctx.stroke();
      // Top-right diagonal
      ctx.beginPath();
      ctx.moveTo(bx, by - s);
      ctx.lineTo(bx + s * 0.65, by - s * 0.4);
      ctx.lineTo(bx - s * 0.65, by + s * 0.4);
      ctx.stroke();
      // Bottom-right diagonal
      ctx.beginPath();
      ctx.moveTo(bx, by + s);
      ctx.lineTo(bx + s * 0.65, by + s * 0.4);
      ctx.lineTo(bx - s * 0.65, by - s * 0.4);
      ctx.stroke();
    } else {
      // ── Power Off / Disconnected Icon ──────────────────────────────────
      const r = 13;
      // Broken circle arc (gap at top)
      ctx.strokeStyle = '#6b7280';
      ctx.lineWidth = 2.5;
      ctx.lineCap = 'round';
      ctx.beginPath();
      ctx.arc(cx, cy, r, (Math.PI * 2 * 0.15), (Math.PI * 2 * 0.85));
      ctx.stroke();
      // Vertical line at top (power button style)
      ctx.beginPath();
      ctx.moveTo(cx, cy - r + 2);
      ctx.lineTo(cx, cy - 4);
      ctx.stroke();
    }
  }
}

// ── Animation Loop ────────────────────────────────────────────────────────────
function animationLoop() {
  if (lastPlaying) {
    pulseFactor = Math.sin(Date.now() / 250) * 0.15 + 0.85; // smooth wave
  } else {
    pulseFactor = 0;
  }
  drawRing(lastSessConn, lastSessPlay, lastConnected, lastPlaying, lastGoalMet);
  animationFrameId = requestAnimationFrame(animationLoop);
}
animationLoop();

// Helper to format duration text nicely
function fmtDurText(secs) {
  if (secs < 60) return `${Math.floor(secs)}s`;
  const m = Math.floor(secs / 60);
  if (m < 60) return `${m}m ${Math.floor(secs % 60)}s`;
  const h = Math.floor(m / 60);
  const leftM = m % 60;
  return `${h}h ${leftM}m`;
}

function fmtRemainingText(secs) {
  if (!Number.isFinite(secs) || secs <= 0) return '0m';
  const rounded = Math.max(1, Math.round(secs));
  if (rounded < 60) return `${rounded}s`;
  const totalMins = Math.round(rounded / 60);
  const h = Math.floor(totalMins / 60);
  const m = totalMins % 60;
  return h > 0 ? `${h}h ${m}m` : `${m}m`;
}

async function updateEstimatedTimeLeft(batteryInfo) {
  const estText = document.getElementById('live-est-time');
  if (!estText) return;

  const currentVal = batteryInfo?.left;
  if (currentVal == null) {
    estText.innerHTML = `<span style="color: var(--muted); font-size: 14px; font-weight: 400; font-family: 'Inter', sans-serif;">—</span>`;
    return;
  }

  let estimateSecs = null;
  let estimateNote = `based on ${currentVal}% battery`;

  try {
    const sessions = await invoke('get_sessions');
    const currentSession = Array.isArray(sessions) && sessions.length > 0 ? sessions[0] : null;
    const connectedSecs = Number(currentSession?.connected_secs || 0);
    const startVal = Number(currentSession?.bat_left_connect);

    if (Number.isFinite(connectedSecs) && connectedSecs > 0 && Number.isFinite(startVal)) {
      const drop = startVal - currentVal;
      if (drop > 0) {
        const drainRatePerSec = drop / connectedSecs;
        if (drainRatePerSec > 0) {
          estimateSecs = currentVal / drainRatePerSec;
          estimateNote = `based on this session's drain rate`;
        }
      }
    }
  } catch (e) {
    console.error('get_sessions failed for estimated time left', e);
  }

  if (estimateSecs == null) {
    // Conservative fallback: roughly 6 hours at 100%.
    estimateSecs = currentVal * 216;
  }

  estText.innerHTML = `≈ ${fmtRemainingText(estimateSecs)}<div style="color: var(--muted); font-size: 13px; font-weight: 400; font-family: 'Inter', sans-serif; margin-top: 4px;">${estimateNote}</div>`;
}

// ── Live Dashboard Extras ───────────────────────────────────────────────────────
async function updateLiveDashboardExtras(connected, batteryInfo, totalTodayPlay) {
  const appsContainer = document.getElementById('live-apps-container');
  const factsText = document.getElementById('live-facts-text');

  if (!appsContainer || !factsText) return;

  if (!connected) {
    appsContainer.innerHTML = '<div style="color: var(--muted); font-size: 13px; font-style: italic; text-align: center;">Nothing playing right now</div>';
    factsText.innerHTML = '<span style="color: var(--muted); font-style: italic;">Connect your earbuds to see live session insights here.</span>';
    return;
  }

  // 1. Now Playing Audio Sources
  try {
    const activeApps = await invoke('get_active_audio_apps');
    if (activeApps && activeApps.length > 0) {
      appsContainer.innerHTML = '';
      activeApps.forEach(appName => {
        const div = document.createElement('div');
        div.style.display = 'flex';
        div.style.alignItems = 'center';
        div.style.gap = '8px';
        div.style.padding = '8px 12px';
        div.style.background = 'var(--bg3)';
        div.style.borderRadius = '6px';
        div.innerHTML = `
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" style="color: var(--accent);"><path d="M3 18v-6a9 9 0 0 1 18 0v6"></path><path d="M21 19a2 2 0 0 1-2 2h-1a2 2 0 0 1-2-2v-3a2 2 0 0 1 2-2h3zM3 19a2 2 0 0 0 2 2h1a2 2 0 0 0 2-2v-3a2 2 0 0 0-2-2H3z"></path></svg>
          <span style="font-size: 13px; color: var(--text);">${typeof bdFmtAppName === 'function' ? bdFmtAppName(appName) : appName}</span>
        `;
        appsContainer.appendChild(div);
      });
    } else {
      appsContainer.innerHTML = '<div style="color: var(--muted); font-size: 13px; font-style: italic; text-align: center;">Nothing playing right now</div>';
    }
  } catch (e) {
    console.error('get_active_audio_apps failed', e);
  }

  // 2. Session Quick Facts
  try {
    const sessions = await invoke('get_sessions');
    if (sessions && sessions.length > 0) {
      const currentSession = sessions[0];
      const connTime = fmtDurText(currentSession.connected_secs);

      let batteryDropText = '';
      if (batteryInfo) {
        const startBat = currentSession.bat_left_connect;
        const currentBat = batteryInfo.left;
        if (startBat != null && currentBat != null) {
          const drop = startBat - currentBat;
          if (drop > 0) {
            batteryDropText = ` The battery has dropped by <strong>${drop}%</strong> since connection.`;
          } else if (drop < 0) {
            batteryDropText = ` The battery has charged by <strong>${Math.abs(drop)}%</strong> since connection.`;
          } else {
            batteryDropText = ` The battery level hasn't changed since connection.`;
          }
        }
      }

      factsText.innerHTML = `You've been connected for <strong>${connTime}</strong> in this session.${batteryDropText}`;
    }
  } catch (e) {
    console.error('get_sessions failed in extras', e);
  }

  await updateEstimatedTimeLeft(batteryInfo);

  // 3. Estimated Time Left
  const estText = document.getElementById('live-est-time');
  if (estText) {
    if (batteryInfo && batteryInfo.left != null) {
      // Assume 6 hours (360 minutes) for 100% battery
      const totalMins = Math.round(batteryInfo.left * 3.6);
      const h = Math.floor(totalMins / 60);
      const m = totalMins % 60;
      estText.innerHTML = `≈ ${h}h ${m}m<div style="color: var(--muted); font-size: 13px; font-weight: 400; font-family: 'Inter', sans-serif; margin-top: 4px;">based on ${batteryInfo.left}%</div>`;
    } else {
      estText.innerHTML = `<span style="color: var(--muted); font-size: 14px; font-weight: 400; font-family: 'Inter', sans-serif;">—</span>`;
    }
  }

  await updateEstimatedTimeLeft(batteryInfo);

  // 4. Daily Goal Progress
  const goalTargetSecs = currentGoal * 3600;
  const progressPct = Math.min(100, (totalTodayPlay / goalTargetSecs) * 100);

  const goalValEl = document.getElementById('live-goal-val');
  const goalTargetEl = document.getElementById('live-goal-target');
  const goalBarEl = document.getElementById('live-goal-bar');
  const goalPctEl = document.getElementById('live-goal-pct');

  if (goalValEl && goalTargetEl && goalBarEl) {
    goalValEl.textContent = fmtDurText(totalTodayPlay);
    goalTargetEl.textContent = `${currentGoal}h`;
    goalBarEl.style.width = `${progressPct}%`;
    if (progressPct >= 100) {
      goalBarEl.style.background = 'linear-gradient(90deg, #f59e0b, #fbbf24)'; // Gold if met
    } else {
      goalBarEl.style.background = 'linear-gradient(90deg, var(--accent), #8b5cf6)';
    }
    if (goalPctEl) {
      goalPctEl.textContent = `${Math.round(progressPct)}%`;
      goalPctEl.style.color = progressPct >= 100 ? '#111827' : 'rgba(255,255,255,0.72)';
      goalPctEl.style.textShadow = progressPct >= 100 ? '0 1px 0 rgba(255,255,255,0.35)' : 'none';
      goalPctEl.style.fontWeight = progressPct >= 100 ? '700' : '600';
    }
  }
}

// ── Snapshot refresh ──────────────────────────────────────────────────────────
async function refreshSnapshot() {
  let snap;
  try {
    snap = await invoke('get_snapshot');
  } catch (e) {
    console.error('get_snapshot failed', e);
    return;
  }

  const { connected, playing, sess_conn, sess_play, today, week, month, lifetime } = snap;
  updateGraphDurationState(connected);

  // Fetch battery status if connected
  let batteryInfo = null;
  if (connected) {
    try {
      batteryInfo = await invoke('get_device_battery');
    } catch (e) {
      console.error('get_device_battery failed', e);
    }
  }
  updateBatteryUI(batteryInfo);

  const snapshotTodayPlay = Number(today.playback) || 0;
  const cachedTodayPlay = getCachedTodayPlaybackSecs();
  const totalTodayPlay = Math.max(snapshotTodayPlay, cachedTodayPlay);
  if (snapshotTodayPlay > 0 || totalTodayPlay > 0) {
    storeTodayPlaybackSecs(totalTodayPlay);
  }
  const goalMet = totalTodayPlay >= (currentGoal * 3600);
  const connectionChanged = wasConnected !== null && wasConnected !== connected;

  // Update animation state variables
  lastSessConn = sess_conn;
  lastSessPlay = sess_play;
  lastConnected = connected;
  lastPlaying = playing;
  lastGoalMet = goalMet;

  // Times
  document.getElementById('conn-time').textContent = fmtFull(sess_conn);

  const playEl = document.getElementById('play-time');
  playEl.textContent = fmtFull(sess_play);

  if (playing) {
    playEl.style.color = goalMet ? '#fbbf24' : 'var(--green)';
  } else {
    playEl.style.color = connected ? '#888' : '#555';
  }

  // Handle connection desktop notifications
  if (wasConnected !== null && wasConnected !== connected) {
    if (notificationsEnabled) {
      if (connected) {
        invoke('show_notification', {
          title: `${currentDeviceName} Connected`,
          body: "Connection active. Ready for streaming."
        }).catch(console.error);
      } else {
        invoke('show_notification', {
          title: `${currentDeviceName} Disconnected`,
          body: "Connection closed. Session stats saved."
        }).catch(console.error);
      }
    }
  }
  wasConnected = connected;
  if (connectionChanged && document.getElementById('page-battery')?.classList.contains('active')) {
    loadBatteryGraph().catch(console.error);
  }

  // Status card
  const dot = document.getElementById('status-dot');
  const title = document.getElementById('status-title');
  const desc = document.getElementById('status-desc');
  if (playing) {
    if (dot) dot.className = 'status-dot playing';
    title.textContent = goalMet ? 'Playing (Goal Met!)' : 'Playing';
    title.style.color = goalMet ? '#fbbf24' : 'var(--green)';
    desc.textContent = 'Streaming live audio';
  } else if (connected) {
    if (dot) dot.className = 'status-dot connected';
    title.textContent = 'Connected (Idle)';
    title.style.color = '#d4a017';
    desc.textContent = 'Link active – no media playing';
  } else {
    if (dot) dot.className = 'status-dot disconnected';
    title.textContent = 'Disconnected';
    title.style.color = 'var(--muted)';
    desc.textContent = `${currentDeviceName} is out of range or off`;
  }

  // Update Live Dashboard Extras
  updateLiveDashboardExtras(connected, batteryInfo, totalTodayPlay);

  // Stats page
  document.getElementById('s-today-conn').textContent = fmtH(today.connected);
  document.getElementById('s-today-play').textContent = fmtH(today.playback);
  document.getElementById('s-week-conn').textContent = fmtH(week.connected);
  document.getElementById('s-week-play').textContent = fmtH(week.playback);
  document.getElementById('s-month-conn').textContent = fmtH(month.connected);
  document.getElementById('s-month-play').textContent = fmtH(month.playback);
  document.getElementById('s-life-conn').textContent = fmtH(lifetime.connected);
  document.getElementById('s-life-play').textContent = fmtH(lifetime.playback);

  // If stats page is active, update chart
  const activePage = document.querySelector('.nav-item.active').dataset.page;
  if (activePage === 'statistics') {
    updateDailyStatsAndChart(totalTodayPlay);
  }
}

// ── Daily Stats & Chart ───────────────────────────────────────────────────────
async function updateDailyStatsAndChart(todayPlaybackSecs = null) {
  await ensureStatsHistoryBounds();
  if (statsMaxWeekOffset !== null) {
    statsWeekOffset = Math.min(statsWeekOffset, statsMaxWeekOffset);
  }
  let history = [];
  try {
    history = await invoke('get_daily_history', { weekOffset: statsWeekOffset });
  } catch (e) {
    console.error('get_daily_history failed', e);
  }
  updateStatsWeekControls();
  drawDailyChart(history, statsWeekOffset);

  if (statsWeekOffset === 0) {
    calculateStreak(history, todayPlaybackSecs);
  } else {
    let currentWeekHistory = [];
    try {
      currentWeekHistory = await invoke('get_daily_history', { weekOffset: 0 });
    } catch (e) {
      console.error('get_daily_history current week failed', e);
    }
    calculateStreak(currentWeekHistory, todayPlaybackSecs);
  }
  updateStatsWeekControls();
}

function calculateStreak(history, todayPlaybackSecs = null) {
  if (!history || history.length === 0) {
    document.getElementById('streak-badge-container').style.display = 'none';
    return;
  }

  const goalSecs = currentGoal * 3600;
  const playbackMap = new Map();
  history.forEach(row => {
    playbackMap.set(row.day, row.playback_secs || 0);
  });

  const getLocalDateString = (d) => {
    const y = d.getFullYear();
    const m = String(d.getMonth() + 1).padStart(2, '0');
    const day = String(d.getDate()).padStart(2, '0');
    return `${y}-${m}-${day}`;
  };

  let today = new Date();
  let yesterday = new Date();
  yesterday.setDate(today.getDate() - 1);

  const todayStr = getLocalDateString(today);
  const yesterdayStr = getLocalDateString(yesterday);

  if (todayPlaybackSecs != null && Number.isFinite(todayPlaybackSecs)) {
    playbackMap.set(todayStr, Math.max(playbackMap.get(todayStr) || 0, todayPlaybackSecs));
  }

  let streak = 0;
  let checkDate = new Date();

  const todayPlay = playbackMap.get(todayStr) || 0;
  const yestPlay = playbackMap.get(yesterdayStr) || 0;

  if (todayPlay >= goalSecs) {
    streak = 1;
    checkDate.setDate(today.getDate() - 1);
  } else if (yestPlay >= goalSecs) {
    streak = 1;
    checkDate.setDate(yesterday.getDate() - 1);
  } else {
    document.getElementById('streak-badge-container').style.display = 'none';
    return;
  }

  while (true) {
    const dateStr = getLocalDateString(checkDate);
    const playTime = playbackMap.get(dateStr) || 0;
    if (playTime >= goalSecs) {
      streak++;
      checkDate.setDate(checkDate.getDate() - 1);
    } else {
      break;
    }
  }

  if (streak > 0) {
    document.getElementById('streak-count').textContent = streak;
    const streakLabel = document.getElementById('streak-label');
    if (streakLabel) {
      streakLabel.textContent = streak === 1 ? 'Day Streak' : 'Days Streak';
    }
    document.getElementById('streak-badge-container').style.display = 'inline-flex';
  } else {
    document.getElementById('streak-badge-container').style.display = 'none';
  }
}

function drawDailyChart(history, weekOffset = 0) {
  const chartCanvas = document.getElementById('stats-chart');
  if (!chartCanvas) return;
  bindStatsChartHover();

  // Make canvas responsive and sharp for high-DPI displays
  const parent = chartCanvas.parentElement;
  const dpr = window.devicePixelRatio || 1;
  const w = parent.clientWidth || 600;
  const h = parent.clientHeight || 180;

  chartCanvas.width = w * dpr;
  chartCanvas.height = h * dpr;
  chartCanvas.style.width = w + 'px';
  chartCanvas.style.height = h + 'px';

  const c = chartCanvas.getContext('2d');
  c.scale(dpr, dpr);

  c.clearRect(0, 0, w, h);

  const days = buildStatsDays(weekOffset);

  const historyMap = new Map();
  if (history) {
    history.forEach(row => historyMap.set(row.day, row));
  }

  days.forEach(day => {
    const match = historyMap.get(day.dateStr);
    if (match) {
      day.connected = match.connected_secs;
      day.playback = match.playback_secs;
    }
  });

  const maxSecs = Math.max(3600, ...days.map(d => Math.max(d.connected, d.playback)));

  const chartBottom = h - 30;
  const chartTop = 20;
  const chartHeight = chartBottom - chartTop;

  const barWidth = 32;
  const gap = Math.max(8, (w - (barWidth * 7)) / 8);
  const goalSecs = currentGoal * 3600;

  c.strokeStyle = '#2a2a2f';
  c.lineWidth = 1;
  c.font = '10px "Segoe UI", sans-serif';
  c.fillStyle = '#888898';

  const gridSegments = 5;
  for (let i = 0; i <= gridSegments; i++) {
    const y = chartTop + (chartHeight * i / gridSegments);
    c.beginPath();
    c.moveTo(gap, y);
    c.lineTo(w - gap, y);
    c.stroke();

    const labelVal = maxSecs - (maxSecs * i / gridSegments);
    c.fillText(fmtH(labelVal), 10, y + 4);
  }

  const hitBoxes = [];
  days.forEach((day, index) => {
    const x = gap + index * (barWidth + gap);
    const connHeight = (day.connected / maxSecs) * chartHeight;
    const playHeight = (day.playback / maxSecs) * chartHeight;

    const yConn = chartBottom - connHeight;
    const yPlay = chartBottom - playHeight;

    if (day.connected > 0) {
      c.fillStyle = 'rgba(255, 255, 255, 0.15)';
      c.beginPath();
      c.roundRect(x, yConn, barWidth, connHeight, [4, 4, 0, 0]);
      c.fill();
    }

    const goalMet = day.playback >= currentGoal * 3600;
    if (day.playback > 0) {
      c.fillStyle = goalMet ? '#fbbf24' : '#4ade80';
      c.beginPath();
      c.roundRect(x, yPlay, barWidth, playHeight, [4, 4, 0, 0]);
      c.fill();
    }

    if (statsChartState.hoverIndex === index) {
      c.fillStyle = 'rgba(255, 255, 255, 0.05)';
      c.beginPath();
      c.roundRect(x - 4, chartTop - 2, barWidth + 8, chartHeight + 4, 10);
      c.fill();
      c.strokeStyle = 'rgba(255, 255, 255, 0.32)';
      c.beginPath();
      c.roundRect(x - 4, chartTop - 2, barWidth + 8, chartHeight + 4, 10);
      c.stroke();
      c.strokeStyle = '#2a2a2f';
    }

    c.fillStyle = '#888898';
    c.font = '11px "Segoe UI", sans-serif';
    c.textAlign = 'center';
    c.fillText(day.label, x + barWidth / 2, chartBottom + 18);

    hitBoxes.push({
      x0: x - 8,
      x1: x + barWidth + 8,
      y0: chartTop - 6,
      y1: chartBottom + 20,
    });
  });

  statsChartState.history = history || [];
  statsChartState.layout = {
    days,
    hitBoxes,
    goalSecs,
    maxSecs,
  };
}

function formatBatteryAgo(ms) {
  if (!ms) return '0 sec ago';

  const seconds = Math.max(0, Math.floor((Date.now() - ms) / 1000));
  const interval = Math.max(1, batteryPollIntervalSec || 10);
  const stepSeconds = seconds < 60 ? Math.floor(seconds / interval) * interval : Math.floor(seconds / 60) * 60;

  if (stepSeconds < 60) {
    return `${stepSeconds} sec${stepSeconds === 1 ? '' : 's'} ago`;
  }

  const minutes = Math.floor(stepSeconds / 60);
  if (minutes < 60) {
    return `${minutes} min${minutes === 1 ? '' : 's'} ago`;
  }

  const hours = Math.floor(minutes / 60);
  if (hours < 24) {
    return `${hours} hr${hours === 1 ? '' : 's'} ${minutes % 60} min ago`;
  }

  const days = Math.floor(hours / 24);
  return `${days} day${days === 1 ? '' : 's'} ago`;
}

function updateBatteryFreshnessLabel() {
  const badge = document.getElementById('bat-last-updated');
  if (!badge) return;

  if (!lastBatteryUpdateAt) {
    badge.style.display = 'none';
    return;
  }

  badge.style.display = 'inline-flex';
  badge.textContent = `Battery Last Updated ${formatBatteryAgo(lastBatteryUpdateAt)}`;
}

function updateBatteryUI(batteryInfo) {
  const card = document.getElementById('battery-card');
  if (!card) return;

  // Always show the battery card on the dashboard
  card.style.display = 'block';

  let isLive = false;
  if (batteryInfo) {
    isLive = true;
    lastBatteryUpdateAt = batteryInfo.updated_at || Date.now();
    localStorage.setItem('last-battery-update-at', String(lastBatteryUpdateAt));
    localStorage.setItem('last-battery-info', JSON.stringify(batteryInfo));
  } else {
    const cached = localStorage.getItem('last-battery-info');
    if (cached) {
      try {
        batteryInfo = JSON.parse(cached);
      } catch (e) {
        console.error('Failed to parse cached battery info', e);
      }
    }
  }

  // Update dynamic device title in Nothing-style uppercase
  const titleEl = document.getElementById('battery-title-device');
  if (titleEl) {
    let name = currentDeviceName.toUpperCase();
    if (name.startsWith("CMF ")) {
      name = name.slice(4);
    }
    titleEl.textContent = name;
  }

  updateBatteryFreshnessLabel();

  // Update subtitle status
  const subEl = document.getElementById('battery-status-sub');
  if (subEl) {
    subEl.textContent = isLive ? 'Connected' : 'Disconnected (Last Known)';
    subEl.style.color = isLive ? 'var(--green)' : 'var(--muted)';
  }

  const updateItem = (colId, valId, progressId, trackId, val, charging) => {
    const colEl = document.getElementById(colId);
    const valEl = document.getElementById(valId);
    const progressEl = document.getElementById(progressId);
    const trackEl = trackId ? document.getElementById(trackId) : null;
    if (!colEl || !valEl || !progressEl) return;

    if (val === null || val === undefined) {
      valEl.textContent = '—';
      progressEl.style.width = '0%';
      colEl.classList.add('offline');
      if (trackEl) trackEl.classList.remove('charging');
      colEl.classList.remove('charging');
    } else {
      valEl.textContent = `${val}%`;
      progressEl.style.width = `${val}%`;

      if (!isLive) {
        colEl.classList.add('offline');
        if (trackEl) trackEl.classList.remove('charging');
        colEl.classList.remove('charging');
      } else {
        colEl.classList.remove('offline');
        if (charging) {
          if (trackEl) {
            trackEl.classList.add('charging');
            const splitEl = document.getElementById('bat-case-split');
            if (splitEl) {
              splitEl.style.width = `${val}%`;
            }
          } else {
            colEl.classList.add('charging');
          }
        } else {
          if (trackEl) trackEl.classList.remove('charging');
          colEl.classList.remove('charging');
        }
      }
    }
  };

  const leftVal = batteryInfo ? batteryInfo.left : null;
  const leftChar = batteryInfo ? batteryInfo.left_charging : false;
  const rightVal = batteryInfo ? batteryInfo.right : null;
  const rightChar = batteryInfo ? batteryInfo.right_charging : false;
  const caseVal = batteryInfo ? batteryInfo.case : null;
  const caseChar = batteryInfo ? batteryInfo.case_charging : false;

  updateItem('bat-left-container', 'bat-left-val', 'bat-left-progress', null, leftVal, leftChar);
  updateItem('bat-case-container', 'bat-case-val', 'bat-case-progress', 'bat-case-track', caseVal, caseChar);
  updateItem('bat-right-container', 'bat-right-val', 'bat-right-progress', null, rightVal, rightChar);
}

// ── History ───────────────────────────────────────────────────────────────────
function fmtSessBattery(r) {
  const hasConnect = r.bat_left_connect !== null || r.bat_right_connect !== null || r.bat_case_connect !== null;
  const hasDisc = r.bat_left_disc !== null || r.bat_right_disc !== null || r.bat_case_disc !== null;
  if (!hasConnect && !hasDisc) return '—';

  const interrupted = r.interrupted === 1 || r.interrupted === true;

  const formatBud = (conn, disc) => {
    const resolvedConn = conn ?? disc;
    const resolvedDisc = disc ?? conn;
    if (resolvedConn === null && resolvedDisc === null) return '—';
    const connStr = resolvedConn !== null ? `${resolvedConn}%` : '—';
    const discStr = resolvedDisc !== null ? `${resolvedDisc}%` : '—';
    if (disc === null && conn !== null && interrupted) {
      return `${connStr} → <span style="color:#f87171;font-size:0.7rem;">${discStr} (last)</span>`;
    }
    return `${connStr} → ${discStr}`;
  };

  return `<div class="mono" style="font-size:0.75rem;line-height:1.3;white-space:nowrap;">
    L: ${formatBud(r.bat_left_connect, r.bat_left_disc)}<br/>
    R: ${formatBud(r.bat_right_connect, r.bat_right_disc)}<br/>
    C: ${formatBud(r.bat_case_connect, r.bat_case_disc)}
  </div>`;
}

async function loadHistory() {
  let rows;
  try { rows = await invoke('get_sessions'); }
  catch (e) { return; }

  const tbody = document.getElementById('history-body');
  tbody.innerHTML = '';
  rows.forEach(r => {
    const tr = document.createElement('tr');
    const start = prettyTimestamp(r.session_start);
    const end = r.session_end ? prettyTimestamp(r.session_end) : '—';
    const interrupted = r.interrupted === 1 || r.interrupted === true;
    const redStyle = interrupted ? 'color:#f87171;' : '';
    tr.innerHTML = `
      <td style="${redStyle}">${start}${interrupted ? ' ⚠' : ''}</td>
      <td style="${redStyle}">${end}</td>
      <td style="${redStyle}">${fmtH(r.connected_secs)}</td>
      <td style="${redStyle || 'color:var(--green)'}">${fmtH(r.playback_secs)}</td>
      <td>${fmtSessBattery(r)}</td>
    `;
    if (interrupted) tr.title = 'Session was interrupted (app closed unexpectedly)';
    tbody.appendChild(tr);
  });
}

document.getElementById('refresh-btn').addEventListener('click', loadHistory);

// ── Settings – reset (2-step: confirm → Windows auth) ────────────────────────
const dialog = document.getElementById('dialog');
const authDialog = document.getElementById('auth-dialog');
const authPwdInput = document.getElementById('auth-password-input');
const authError = document.getElementById('auth-error');
const authUsername = document.getElementById('auth-username');

// Populate username label
if (authUsername) {
  // Tauri doesn't expose env vars to JS, so we show a generic label.
  // The Rust side uses $env:USERNAME automatically.
  authUsername.textContent = 'your Windows account';
}

// Step 1 – show confirm dialog
document.getElementById('reset-btn').addEventListener('click', () => {
  dialog.hidden = false;
});

// Step 1 – cancel
document.getElementById('dialog-cancel').addEventListener('click', () => {
  dialog.hidden = true;
});

// Step 1 – "Continue →" opens the password dialog
document.getElementById('dialog-confirm').addEventListener('click', () => {
  dialog.hidden = true;
  if (authPwdInput) authPwdInput.value = '';
  if (authError) authError.style.display = 'none';
  authDialog.hidden = false;
  // Auto-focus the password field
  setTimeout(() => authPwdInput && authPwdInput.focus(), 60);
});

// Step 2 – cancel
document.getElementById('auth-cancel').addEventListener('click', () => {
  authDialog.hidden = true;
  if (authPwdInput) authPwdInput.value = '';
  if (authError) authError.style.display = 'none';
});

// Step 2 – confirm: verify password then reset
document.getElementById('auth-confirm').addEventListener('click', async () => {
  const pwd = authPwdInput ? authPwdInput.value : '';
  const confirmBtn = document.getElementById('auth-confirm');
  confirmBtn.disabled = true;
  confirmBtn.textContent = 'Verifying…';

  let ok = false;
  try {
    ok = await invoke('verify_windows_password', { password: pwd });
  } catch (e) {
    console.error('verify_windows_password failed', e);
  } finally {
    confirmBtn.disabled = false;
    confirmBtn.textContent = 'Confirm Reset';
  }

  if (ok) {
    authDialog.hidden = true;
    if (authPwdInput) authPwdInput.value = '';
    try {
      await invoke('reset_all');
      localStorage.removeItem('daily-playback-cache-date');
      localStorage.removeItem('daily-playback-cache-secs');
      if (resetSuccessMsg) {
        resetSuccessMsg.textContent = 'All session history and statistics have been reset.';
      }
      if (resetSuccessDialog) resetSuccessDialog.hidden = false;
      try {
        await refreshSnapshot();
      } catch (refreshErr) {
        console.error('refreshSnapshot after reset failed', refreshErr);
      }
    } catch (e) { console.error('reset_all failed', e); }
  } else {
    if (authPwdInput) authPwdInput.value = '';
    if (authError) authError.style.display = 'block';
    setTimeout(() => authPwdInput && authPwdInput.focus(), 60);
  }
});

// Allow pressing Enter inside the password input
authPwdInput && authPwdInput.addEventListener('keydown', (e) => {
  if (e.key === 'Enter') document.getElementById('auth-confirm').click();
});

// ── Tauri event: backend pushed state-changed ─────────────────────────────────
event.listen('state-changed', () => refreshSnapshot());

// ── Polling timer (1 s) as safety net ────────────────────────────────────────
setInterval(refreshSnapshot, 1000);

// ── Initial draw ──────────────────────────────────────────────────────────────
drawRing(0, 0, false, false, false);
refreshSnapshot();

// ── Redraw chart on window resize ─────────────────────────────────────────────
window.addEventListener('resize', () => {
  const activePage = document.querySelector('.nav-item.active').dataset.page;
  if (activePage === 'statistics') {
    updateDailyStatsAndChart();
  }
});

// ═══════════════════════════════════════════════════════════════════════════════
// ── SESSION BREAKDOWN PAGE ────────────────────────────────────────────────────
// ═══════════════════════════════════════════════════════════════════════════════

let bdAllSessions = [];      // full list from backend
let bdSelectedId = null;    // currently selected session id
let bdNoteTimer = null;    // debounce timer for note autosave

// ── Format helpers ────────────────────────────────────────────────────────────
function bdFmtDur(secs) {
  if (!secs || secs <= 0) return '—';
  return fmtH(secs);
}

function bdFmtDate(ts) {
  if (!ts) return '—';
  const parsed = new Date(String(ts).replace(' ', 'T'));
  if (!Number.isNaN(parsed.getTime())) {
    return new Intl.DateTimeFormat('en-US', {
      year: 'numeric',
      month: 'short',
      day: 'numeric',
      hour: 'numeric',
      minute: '2-digit',
      second: '2-digit',
      hour12: true,
    }).format(parsed);
  }
  return String(ts).replace('T', ' ');
}

function bdFmtBat(v) {
  return v !== null && v !== undefined ? `${v}%` : '—';
}

function bdFmtAppName(name) {
  if (!name) return '—';

  // Custom name map for non-user-friendly app names
  const nameMap = {
    'msedgewebview2': 'WebView2 (App Container)',
    'ShellExperienceHost': 'Windows Shell Experience',
  };

  let formatted = name.replace(/\./g, ' ');
  return nameMap[name] || nameMap[formatted] || formatted;
}

// ── Load the session list from backend ────────────────────────────────────────
async function bdLoadSessions() {
  try {
    bdAllSessions = await invoke('get_sessions_for_breakdown');
  } catch (e) {
    console.error('get_sessions_for_breakdown failed', e);
    bdAllSessions = [];
  }
  bdRenderList();
  // Auto-load the most recent session (first after sort)
  const picker = document.getElementById('bd-session-picker');
  if (picker && picker.options.length > 1) {
    picker.selectedIndex = 1; // index 0 is placeholder
    const firstId = parseInt(picker.value);
    if (!isNaN(firstId)) bdSelectSession(firstId);
  }
}

// ── Populate the session picker dropdown ─────────────────────────────────────
function bdRenderList() {
  const q = (document.getElementById('bd-search')?.value || '').toLowerCase();
  const sort = document.getElementById('bd-sort')?.value || 'newest';

  let filtered = bdAllSessions.filter(s => {
    if (q) {
      if (!s.session_start.toLowerCase().includes(q)) return false;
    }
    return true;
  });

  filtered.sort((a, b) => {
    if (sort === 'oldest') return a.session_start.localeCompare(b.session_start);
    if (sort === 'time-high') return b.connected_secs - a.connected_secs;
    if (sort === 'time-low') return a.connected_secs - b.connected_secs;
    return b.session_start.localeCompare(a.session_start); // newest
  });

  const picker = document.getElementById('bd-session-picker');
  if (!picker) return;

  const prevId = bdSelectedId;
  picker.innerHTML = '<option value="">— Select a session —</option>';

  filtered.forEach(s => {
    const opt = document.createElement('option');
    opt.value = s.id;
    const date = bdFmtDate(s.session_start);
    const conn = bdFmtDur(s.connected_secs).padEnd(10, '\u00A0');
    const play = bdFmtDur(s.playback_secs);
    const tag = (s.interrupted === 1 || s.interrupted === true) ? '\u00A0\u00A0⚠' : '';
    opt.textContent = `${date}\u00A0\u00A0│\u00A0\u00A0⇄\u00A0Conn:\u00A0${conn}\u00A0\u00A0│\u00A0\u00A0▶\u00A0Play:\u00A0${play}${tag}`;
    if (s.id === prevId) opt.selected = true;
    picker.appendChild(opt);
  });
}

// ── Select a session and load its full breakdown ───────────────────────────────
async function bdSelectSession(id) {
  bdSelectedId = id;

  // Highlight selected item
  document.querySelectorAll('.bd-session-item').forEach(el => {
    el.classList.toggle('active', parseInt(el.dataset.id) === id);
  });

  const detail = document.getElementById('bd-detail');
  if (!detail) return;
  detail.innerHTML = '<div class="bd-empty-state" style="height:60px;">Loading…</div>';

  let bd;
  try {
    bd = await invoke('get_session_breakdown', { sessionId: id });
  } catch (e) {
    console.error('get_session_breakdown failed', e);
    return;
  }
  if (!bd) {
    detail.innerHTML = '<div class="bd-empty-state">No data for this session.</div>';
    return;
  }

  bdRenderDetail(bd, detail);
}

// ── Render the full detail panel ──────────────────────────────────────────────
function bdRenderDetail(bd, container) {
  const s = bd.session;

  // Normalise app durations: a single app cannot play longer than the total session playback.
  // This resolves minor discrepancies caused by background thread polling desynchronization.
  if (bd.app_totals) {
    bd.app_totals.forEach(t => {
      if (t.total_secs > s.playback_secs) {
        t.total_secs = s.playback_secs;
      }
    });
    // Re-sort after capping just in case
    bd.app_totals.sort((a, b) => b.total_secs - a.total_secs);
  }

  // Battery drain values
  const batL0 = s.bat_left_connect, batL1 = s.bat_left_disc;
  const batR0 = s.bat_right_connect, batR1 = s.bat_right_disc;
  const batC0 = s.bat_case_connect, batC1 = s.bat_case_disc;

  const drainStr = (a, b) => {
    if (a == null || b == null) return '—';
    const d = a - b;
    return `${a}% → ${b}% (${d >= 0 ? '-' : '+'}${Math.abs(d)}%)`;
  };

  // Most used app
  const topApp = bd.app_totals.length > 0 ? bd.app_totals[0] : null;

  const interruptedBanner = (s.interrupted === 1 || s.interrupted === true)
    ? `<div style="background:rgba(248,113,113,0.12);border:1px solid rgba(248,113,113,0.35);border-radius:8px;padding:10px 14px;margin-bottom:12px;display:flex;align-items:center;gap:10px;">
        <span style="font-size:18px;">⚠️</span>
        <div>
          <div style="color:#f87171;font-weight:600;font-size:13px;">Interrupted Session</div>
          <div style="color:#fca5a5;font-size:11px;margin-top:2px;">The app was closed unexpectedly during this session. Times shown are the last recorded values before shutdown.</div>
        </div>
       </div>`
    : '';

  container.innerHTML = `
    ${interruptedBanner}
    <!-- ── Summary card ── -->
    <div class="card">
      <div class="bd-section-title">Session Summary</div>
      <div class="bd-info-grid">
        <div class="bd-info-item">
          <span class="bd-info-label">Start</span>
          <span class="bd-info-value" style="font-size:12px;">${bdFmtDate(s.session_start)}</span>
        </div>
        <div class="bd-info-item">
          <span class="bd-info-label">End</span>
          <span class="bd-info-value" style="font-size:12px;">${s.session_end ? bdFmtDate(s.session_end) : '—'}</span>
        </div>
        <div class="bd-info-item">
          <span class="bd-info-label">Connected</span>
          <span class="bd-info-value">${bdFmtDur(s.connected_secs)}</span>
        </div>
        <div class="bd-info-item">
          <span class="bd-info-label">Playback</span>
          <span class="bd-info-value green">${bdFmtDur(s.playback_secs)}</span>
        </div>
        <div class="bd-info-item">
          <span class="bd-info-label">Left Earbud</span>
          <span class="bd-info-value" style="font-size:12px;">${drainStr(batL0, batL1)}</span>
        </div>
        <div class="bd-info-item">
          <span class="bd-info-label">Right Earbud</span>
          <span class="bd-info-value" style="font-size:12px;">${drainStr(batR0, batR1)}</span>
        </div>
        <div class="bd-info-item">
          <span class="bd-info-label">Case</span>
          <span class="bd-info-value" style="font-size:12px;">${drainStr(batC0, batC1)}</span>
        </div>
        <div class="bd-info-item">
          <span class="bd-info-label">Top App</span>
          <span class="bd-info-value" style="font-size:12px;">${topApp ? bdFmtAppName(topApp.process_name) + ' · ' + bdFmtDur(topApp.total_secs) : '—'}</span>
        </div>
      </div>
    </div>

    <!-- ── App usage bars ── -->
    <div class="card" id="bd-apps-card">
      <div class="bd-section-title">App Audio Usage (${bd.app_totals.length} apps)</div>
      <div class="bd-app-list" id="bd-app-list"></div>
    </div>

    <!-- ── Battery drain curve ── -->
    <div class="card" id="bd-bat-card" style="${(batL0 == null && batR0 == null && batC0 == null) ? 'display:none' : ''}">
      <div style="display:flex;align-items:center;justify-content:space-between;margin-bottom:10px;">
        <div class="bd-section-title" style="margin:0;">Battery Drain Curve</div>
        <select id="bd-bat-series" class="input-field" style="width:180px;font-size:12px;padding:5px 10px;">
          <option value="buds">Buds (Left + Right avg)</option>
          <option value="case">Case</option>
        </select>
      </div>
      <div class="bd-battery-canvas-wrap">
        <canvas id="bd-bat-canvas"></canvas>
      </div>
    </div>

  `;

  // Render app bars
  bdRenderAppBars(bd.app_totals, s.playback_secs);

  // Render battery drain canvas
  if (batL0 != null || batR0 != null || batC0 != null) {
    window._bdLastSession = s;
    requestAnimationFrame(() => bdDrawBatteryCurve(s));
  }

}

// ── Render horizontal app usage bars ──────────────────────────────────────────
function bdRenderAppBars(totals, sessionPlaySecs = 1) {
  const list = document.getElementById('bd-app-list');
  if (!list) return;
  if (totals.length === 0) {
    list.innerHTML = '<div style="color:var(--muted);font-size:12px;">No app audio data recorded for this session.<br><span style="font-size:11px;">App tracking is active from the next session onward.</span></div>';
    return;
  }
  const denominator = Math.max(sessionPlaySecs, 1);
  list.innerHTML = '';
  totals.forEach((t, i) => {
    const pct = Math.min((t.total_secs / denominator) * 100, 100).toFixed(1);
    const card = document.createElement('div');
    card.className = 'bd-app-row';
    const formattedName = bdFmtAppName(t.process_name);
    card.innerHTML = `
      <div class="bd-app-row-header">
        <span class="bd-app-name">${formattedName}</span>
        <span class="bd-app-dur">${bdFmtDur(t.total_secs)}</span>
      </div>
      <div class="bd-app-bar-track">
        <div class="bd-app-bar-fill${i === 0 ? ' top' : ''}" style="width:0%"></div>
      </div>
      <span class="bd-app-rank">${pct}% of session</span>
    `;
    list.appendChild(card);
    // Animate bar width after paint
    requestAnimationFrame(() => {
      const fill = card.querySelector('.bd-app-bar-fill');
      if (fill) fill.style.width = pct + '%';
    });
  });
}

// ── Draw battery drain line chart on canvas ───────────────────────────────────
function bdDrawBatteryCurve(s, mode) {
  const canvas = document.getElementById('bd-bat-canvas');
  if (!canvas) return;
  const wrap = canvas.parentElement;
  const dpr = window.devicePixelRatio || 1;
  const W = wrap.clientWidth || 400;
  const H = 240;
  canvas.width = W * dpr;
  canvas.height = H * dpr;
  canvas.style.width = W + 'px';
  canvas.style.height = H + 'px';

  const c = canvas.getContext('2d');
  c.scale(dpr, dpr);
  c.clearRect(0, 0, W, H);

  // Increased top/bottom padding so labels aren't clipped
  const padL = 42, padR = 20, padT = 26, padB = 36;
  const chartW = W - padL - padR;
  const chartH = H - padT - padB;

  // Grid lines
  c.strokeStyle = '#2a2a2f';
  c.lineWidth = 1;
  c.font = '10px "Segoe UI", sans-serif';
  c.fillStyle = '#888898';
  c.textAlign = 'right';
  [0, 25, 50, 75, 100].forEach(pct => {
    const y = padT + chartH - (pct / 100) * chartH;
    c.beginPath();
    c.moveTo(padL, y);
    c.lineTo(padL + chartW, y);
    c.stroke();
    c.fillText(pct + '%', padL - 6, y + 3.5);
  });

  // X labels
  c.textAlign = 'left';
  c.fillStyle = '#888898';
  c.font = '10px "Segoe UI", sans-serif';
  c.fillText('Start', padL, H - 10);
  c.textAlign = 'right';
  c.fillText('End', padL + chartW, H - 10);

  // Determine series based on dropdown mode
  const resolvedMode = mode || document.getElementById('bd-bat-series')?.value || 'buds';
  let series;
  if (resolvedMode === 'case') {
    series = [
      { label: 'Case', v0: s.bat_case_connect, v1: s.bat_case_disc, color: '#fbbf24' },
    ].filter(sr => sr.v0 != null || sr.v1 != null);
  } else {
    // Buds: show L and R individually, with an avg line
    const L0 = s.bat_left_connect, L1 = s.bat_left_disc;
    const R0 = s.bat_right_connect, R1 = s.bat_right_disc;
    const avgStart = (L0 != null && R0 != null) ? Math.round((L0 + R0) / 2) : (L0 ?? R0);
    const avgEnd = (L1 != null && R1 != null) ? Math.round((L1 + R1) / 2) : (L1 ?? R1);
    series = [
      { label: 'L', v0: L0, v1: L1, color: '#6366f1' },
      { label: 'R', v0: R0, v1: R1, color: '#4ade80' },
      { label: 'Avg', v0: avgStart, v1: avgEnd, color: '#f97316', dashed: true },
    ].filter(sr => sr.v0 != null || sr.v1 != null);
  }

  series.forEach(sr => {
    const x0 = padL, x1 = padL + chartW;
    const start = sr.v0 ?? sr.v1 ?? 0;
    const end = sr.v1 ?? sr.v0 ?? 0;
    const y0 = padT + chartH - (start / 100) * chartH;
    const y1 = padT + chartH - (end / 100) * chartH;

    // Gradient fill (skip for avg/dashed)
    if (!sr.dashed) {
      const grad = c.createLinearGradient(x0, y0, x1, y1);
      grad.addColorStop(0, sr.color + '44');
      grad.addColorStop(1, sr.color + '00');
      c.beginPath();
      c.moveTo(x0, y0);
      c.lineTo(x1, y1);
      c.lineTo(x1, padT + chartH);
      c.lineTo(x0, padT + chartH);
      c.closePath();
      c.fillStyle = grad;
      c.fill();
    }

    // Line
    c.beginPath();
    c.moveTo(x0, y0);
    c.lineTo(x1, y1);
    c.strokeStyle = sr.color;
    c.lineWidth = sr.dashed ? 1.5 : 2;
    if (sr.dashed) {
      c.setLineDash([6, 4]);
    } else {
      c.setLineDash([]);
    }
    c.stroke();
    c.setLineDash([]);

    // Dots + labels (only for non-avg series or avg when it's the only one)
    const showLabel = !sr.dashed || series.length === 1;
    [{ x: x0, y: y0, v: start }, { x: x1, y: y1, v: end }].forEach(pt => {
      c.beginPath();
      c.arc(pt.x, pt.y, sr.dashed ? 3 : 4, 0, Math.PI * 2);
      c.fillStyle = sr.color;
      c.fill();
      if (showLabel) {
        c.fillStyle = sr.color;
        c.font = 'bold 9px "Segoe UI", sans-serif';
        c.textAlign = 'center';
        // Clamp label inside top padding
        const labelY = Math.max(padT - 6, pt.y - 10);
        c.fillText(sr.label + ':' + pt.v + '%', pt.x, labelY);
      }
    });
  });
}

// Wire the series dropdown to redraw on change (stored on window for access after render)
window._bdLastSession = null;
document.addEventListener('change', e => {
  if (e.target.id === 'bd-bat-series' && window._bdLastSession) {
    requestAnimationFrame(() => bdDrawBatteryCurve(window._bdLastSession));
  }
});

// ── Export session data ───────────────────────────────────────────────────────
async function bdExport(sessionId, format) {
  let content;
  try {
    content = await invoke('export_session', { sessionId, format });
  } catch (e) {
    console.error('export_session failed', e);
    return;
  }
  if (!content) return;
  const mime = format === 'csv' ? 'text/csv' : 'application/json';
  const ext = format === 'csv' ? 'csv' : 'json';
  const blob = new Blob([content], { type: mime });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = `session_${sessionId}.${ext}`;
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}

// ── Wire up filters & session picker ─────────────────────────────────────────
document.getElementById('bd-search')?.addEventListener('input', bdRenderList);
document.getElementById('bd-sort')?.addEventListener('change', bdRenderList);
document.getElementById('bd-session-picker')?.addEventListener('change', e => {
  const id = parseInt(e.target.value);
  if (!isNaN(id)) bdSelectSession(id);
});

// ── Load breakdown data whenever the nav item is clicked ──────────────────────
document.querySelector('.nav-item[data-page="breakdown"]')
  ?.addEventListener('click', bdLoadSessions);

// ── Battery Analytics Graph ──────────────────────────────────────────────────
let batteryChart = null;
let batteryGraphLoadToken = 0;
const graphTypeOptions = {
  line: 'Line Graph',
  area: 'Area Chart',
  bar: 'Bar Chart',
  pie: 'Pie Chart',
  donut: 'Donut Chart',
  radar: 'Radar Chart'
};

function syncGraphTypeOptions(item) {
  const typeSelect = document.getElementById('graph-type');
  if (!typeSelect) return;

  const currentValue = typeSelect.value || 'line';
  const allowedValues = item === 'all'
    ? ['line', 'area', 'bar', 'pie', 'donut', 'radar']
    : ['line', 'area', 'bar'];

  typeSelect.innerHTML = allowedValues
    .map(value => `<option value="${value}">${graphTypeOptions[value]}</option>`)
    .join('');

  if (!allowedValues.includes(currentValue)) {
    typeSelect.value = 'line';
  } else {
    typeSelect.value = currentValue;
  }
}

async function loadBatteryGraph() {
  const loadToken = ++batteryGraphLoadToken;
  const canvasEl = document.getElementById('battery-graph-canvas');
  const emptyEl = document.getElementById('graph-empty-state');
  if (!canvasEl) return;

  const duration = document.getElementById('graph-duration')?.value || 'week';
  const item = document.getElementById('graph-item')?.value || 'all';
  let chartType = document.getElementById('graph-type')?.value || 'line';

  syncGraphTypeOptions(item);
  const typeSelect = document.getElementById('graph-type');
  chartType = typeSelect?.value || chartType;
  if (item !== 'all' && chartType === 'radar') {
    chartType = 'line';
    if (typeSelect) typeSelect.value = 'line';
  }

  // Get current live state
  let liveLeft = null, liveRight = null, liveCase = null, isLive = false;
  const cached = localStorage.getItem('last-battery-info');
  if (cached) {
    try {
      const batteryInfo = JSON.parse(cached);
      liveLeft = batteryInfo.left;
      liveRight = batteryInfo.right;
      liveCase = batteryInfo.case;
      // Use the global connection state tracker (wasConnected is set by the poll loop)
      // Fallback: check status text but use exact match, not includes(), to avoid
      // matching "Disconnected (Last Known)" which also contains "Connected"
      const statusEl = document.getElementById('battery-status-sub');
      const statusText = statusEl ? statusEl.textContent.trim() : '';
      isLive = statusText === 'Connected' || (typeof lastConnected !== 'undefined' && lastConnected);
    } catch (e) { }
  }

  // Get battery step size from backend
  let batteryStep = 1;
  try {
    batteryStep = await invoke('get_battery_step');
  } catch (e) {
    console.error('get_battery_step failed', e);
  }

  const roundToStep = (val) => {
    if (val == null) return null;
    const clamped = Math.min(100, Math.max(0, val));
    if (batteryStep <= 1) return clamped;
    return Math.min(100, Math.round(clamped / batteryStep) * batteryStep);
  };

  // Fetch data
  let rawData = [];
  try {
    rawData = await invoke('get_battery_graph_data', { duration });
  } catch (e) {
    console.error('get_battery_graph_data failed', e);
  }
  if (loadToken !== batteryGraphLoadToken) return;

  // Handle empty state
  if (!rawData || rawData.length === 0) {
    canvasEl.style.display = 'none';
    if (emptyEl) emptyEl.style.display = 'flex';
    if (batteryChart) {
      batteryChart.destroy();
      batteryChart = null;
    }
    return;
  }

  canvasEl.style.display = 'block';
  if (emptyEl) emptyEl.style.display = 'none';
  canvasEl.style.minHeight = '320px';
  canvasEl.style.height = chartType === 'pie' || chartType === 'donut' ? '380px' : '360px';

  let categories = [];
  let series = [];

  const greenColor = '#4ade80'; // Left Bud  — green
  const blueColor = '#60a5fa'; // Right Bud — blue
  const purpleColor = '#a78bfa'; // Case       — purple
  const whiteColor = '#fbbf24'; // Average    — amber (high contrast on dark)

  let colors = [];

  if (duration === 'session') {
    const pt = rawData[0];
    const leftE = roundToStep(pt.left_end ?? pt.left_start);
    const rightE = roundToStep(pt.right_end ?? pt.right_start);
    const caseE = roundToStep(pt.case_end ?? pt.case_start);
    const ptLeftStart = roundToStep(pt.left_start);
    const ptRightStart = roundToStep(pt.right_start);
    const ptCaseStart = roundToStep(pt.case_start);
    const leftDrain = Math.max(0, (ptLeftStart ?? 0) - (leftE ?? 0));
    const rightDrain = Math.max(0, (ptRightStart ?? 0) - (rightE ?? 0));
    const caseDrain = Math.max(0, (ptCaseStart ?? 0) - (caseE ?? 0));
    const avgDrain = roundToStep((leftDrain + rightDrain) / 2);

    if (chartType === 'pie' || chartType === 'donut') {
      if (item === 'left') {
        series = [leftDrain];
        categories = ['Left Bud'];
        colors = [greenColor];
      } else if (item === 'right') {
        series = [rightDrain];
        categories = ['Right Bud'];
        colors = [blueColor];
      } else if (item === 'case') {
        series = [caseDrain];
        categories = ['Case'];
        colors = [purpleColor];
      } else if (item === 'avg') {
        series = [avgDrain];
        categories = ['Average Bud'];
        colors = [whiteColor];
      } else {
        series = [leftDrain, rightDrain, caseDrain];
        categories = ['Left Bud', 'Right Bud', 'Case'];
        colors = [greenColor, blueColor, purpleColor];
      }
    } else if (chartType === 'radar') {
      const leftTotal = leftDrain;
      const rightTotal = rightDrain;
      const caseTotal = caseDrain;

      if (item === 'left') {
        categories = ['Left Bud'];
        series = [{ name: 'Left Bud Drain', data: [leftTotal] }];
        colors = [greenColor];
      } else if (item === 'right') {
        categories = ['Right Bud'];
        series = [{ name: 'Right Bud Drain', data: [rightTotal] }];
        colors = [blueColor];
      } else if (item === 'case') {
        categories = ['Case'];
        series = [{ name: 'Case Drain', data: [caseTotal] }];
        colors = [purpleColor];
      } else {
        // Radar chart: show only Left Bud Drain, Right Bud Drain, Case Drain (no Average / Max)
        categories = ['Left Bud', 'Right Bud', 'Case'];
        series = [
          {
            name: 'Drain Comparison',
            data: [leftTotal, rightTotal, caseTotal]
          }
        ];
        colors = [greenColor];
      }

    } else { // all
      series = [
        { name: 'Left Bud', data: [ptLeftStart ?? 0, leftE ?? 0] },
        { name: 'Right Bud', data: [ptRightStart ?? 0, rightE ?? 0] },
        { name: 'Case', data: [ptCaseStart ?? 0, caseE ?? 0] }
      ];
      colors = [greenColor, blueColor, purpleColor];
    }

    if (chartType !== 'pie' && chartType !== 'donut' && chartType !== 'radar') {
      categories = ['Start', 'End'];
    }
  } else {
    // Session/day/week/month battery levels plotted across sessions
    const parseGraphTimestamp = (value) => {
      if (!value) return null;
      const parsed = new Date(value);
      return Number.isNaN(parsed.getTime()) ? null : parsed;
    };

    const dayStart = (date) => new Date(date.getFullYear(), date.getMonth(), date.getDate());
    const weekStart = (date) => {
      const d = dayStart(date);
      const dow = d.getDay();
      d.setDate(d.getDate() - dow);
      return d;
    };
    const monthWeekStart = (date) => {
      const d = new Date(date.getFullYear(), date.getMonth(), date.getDate());
      const weekIndex = Math.floor((d.getDate() - 1) / 7);
      d.setDate(1 + weekIndex * 7);
      return dayStart(d);
    };
    const bucketStartFor = (date) => {
      if (!date) return null;
      if (duration === 'day') return new Date(date);
      if (duration === 'week') return weekStart(date);
      return monthWeekStart(date);
    };
    const formatBucketLabel = (date) => {
      if (!date) return 'Unknown';
      if (duration === 'day') {
        return new Intl.DateTimeFormat('en-US', {
          month: 'short',
          day: 'numeric',
          hour: 'numeric',
          minute: '2-digit'
        }).format(date);
      }
      if (duration === 'week') {
        return new Intl.DateTimeFormat('en-US', {
          month: 'short',
          day: 'numeric'
        }).format(date);
      }
      const weekIndex = Math.floor((date.getDate() - 1) / 7) + 1;
      const end = new Date(date);
      end.setDate(Math.min(new Date(date.getFullYear(), date.getMonth() + 1, 0).getDate(), date.getDate() + 6));
      return `W${weekIndex} ${new Intl.DateTimeFormat('en-US', { month: 'short', day: 'numeric' }).format(date)} - ${new Intl.DateTimeFormat('en-US', { month: 'short', day: 'numeric' }).format(end)}`;
    };
    const aggregateBuckets = (rows) => {
      const buckets = new Map();
      rows.forEach(pt => {
        const ts = parseGraphTimestamp(pt.ts || pt.session_start || pt.label);
        const bucketDate = bucketStartFor(ts);
        const key = bucketDate ? bucketDate.getTime() : (pt.label || '');
        if (!buckets.has(key)) {
          buckets.set(key, {
            sortKey: bucketDate ? bucketDate.getTime() : 0,
            label: bucketDate ? formatBucketLabel(bucketDate) : pt.label,
            rowCount: 0,
            left_start: 0,
            left_end: 0,
            right_start: 0,
            right_end: 0,
            case_start: 0,
            case_end: 0
          });
        }
        const bucket = buckets.get(key);
        bucket.rowCount += 1;
        bucket.left_start += pt.left_start ?? 0;
        bucket.left_end += pt.left_end ?? 0;
        bucket.right_start += pt.right_start ?? 0;
        bucket.right_end += pt.right_end ?? 0;
        bucket.case_start += pt.case_start ?? 0;
        bucket.case_end += pt.case_end ?? 0;
      });
      return [...buckets.values()].sort((a, b) => a.sortKey - b.sortKey);
    };

    const toBatteryValue = (value) => {
      if (value == null) return null;
      const num = Number(value);
      return Number.isNaN(num) ? null : Math.min(100, Math.max(0, num));
    };
    const avgOf = (values) => {
      const nums = values.filter(v => v != null);
      if (!nums.length) return null;
      return nums.reduce((a, b) => a + b, 0) / nums.length;
    };

    const leftStarts = rawData.map(pt => toBatteryValue(pt.left_start));
    const leftEnds = rawData.map(pt => toBatteryValue(pt.left_end));
    const rightStarts = rawData.map(pt => toBatteryValue(pt.right_start));
    const rightEnds = rawData.map(pt => toBatteryValue(pt.right_end));
    const caseStarts = rawData.map(pt => toBatteryValue(pt.case_start));
    const caseEnds = rawData.map(pt => toBatteryValue(pt.case_end));
    const avgStarts = rawData.map((pt, idx) => avgOf([leftStarts[idx], rightStarts[idx]]));
    const avgEnds = rawData.map((pt, idx) => avgOf([leftEnds[idx], rightEnds[idx]]));
    const drainFrom = (start, end) => {
      if (start == null && end == null) return null;
      const resolvedStart = start ?? end ?? 0;
      const resolvedEnd = end ?? start ?? 0;
      return Math.max(0, resolvedStart - resolvedEnd);
    };
    const fillMissingSeries = (values) => {
      const filled = [];
      let last = null;
      for (const value of values) {
        if (value != null) {
          last = value;
          filled.push(value);
        } else {
          filled.push(last);
        }
      }
      return filled;
    };
    const fillPair = (starts, ends) => {
      const startSeeded = starts.map((v, idx) => v ?? ends[idx]);
      const endSeeded = ends.map((v, idx) => v ?? starts[idx]);
      return [fillMissingSeries(startSeeded), fillMissingSeries(endSeeded)];
    };
    const [filledLeftStarts, filledLeftEnds] = fillPair(leftStarts, leftEnds);
    const [filledRightStarts, filledRightEnds] = fillPair(rightStarts, rightEnds);
    const [filledCaseStarts, filledCaseEnds] = fillPair(caseStarts, caseEnds);
    const [filledAvgStarts, filledAvgEnds] = fillPair(avgStarts, avgEnds);
    const clampLevel = (value) => value == null ? null : Math.min(100, Math.max(0, value));
    const leftLevels = filledLeftStarts.map((v, idx) => clampLevel(avgOf([v, filledLeftEnds[idx]])));
    const rightLevels = filledRightStarts.map((v, idx) => clampLevel(avgOf([v, filledRightEnds[idx]])));
    const caseLevels = filledCaseStarts.map((v, idx) => clampLevel(avgOf([v, filledCaseEnds[idx]])));
    const avgLevels = filledAvgStarts.map((v, idx) => clampLevel(avgOf([v, filledAvgEnds[idx]])));
    const avgSeriesValue = (values) => {
      const nums = values.filter(v => v != null);
      if (!nums.length) return null;
      return nums.reduce((a, b) => a + b, 0) / nums.length;
    };

    categories = rawData.map(pt => pt.label);

    if (chartType === 'pie' || chartType === 'donut') {
      const totalLeft = roundToStep(avgSeriesValue(leftLevels));
      const totalRight = roundToStep(avgSeriesValue(rightLevels));
      const totalCase = roundToStep(avgSeriesValue(caseLevels));
      const totalAvg = roundToStep(avgSeriesValue(avgLevels));

      if (item === 'left') {
        series = [totalLeft ?? 0];
        categories = ['Left Bud'];
        colors = [greenColor];
      } else if (item === 'right') {
        series = [totalRight ?? 0];
        categories = ['Right Bud'];
        colors = [blueColor];
      } else if (item === 'case') {
        series = [totalCase ?? 0];
        categories = ['Case'];
        colors = [purpleColor];
      } else if (item === 'avg') {
        series = [totalAvg ?? 0];
        categories = ['Average Bud'];
        colors = [whiteColor];
      } else {
        series = [totalLeft ?? 0, totalRight ?? 0, totalCase ?? 0];
        categories = ['Left Bud', 'Right Bud', 'Case'];
        colors = [greenColor, blueColor, purpleColor];
      }
    } else if (chartType === 'radar') {
      const radarDurationLabel = graphDurationSelect?.selectedOptions?.[0]?.textContent || (duration === 'day' ? 'Today' : duration === 'week' ? 'Week' : duration === 'month' ? 'Month' : 'This Session');
      const radarItemLabel = 'All';

      if (duration === 'session') {
        const sessionStart = roundToStep(avgOf([ptLeftStart, ptRightStart, ptCaseStart]) ?? ptLeftStart ?? ptRightStart ?? ptCaseStart ?? 0);
        const sessionEnd = roundToStep(avgOf([leftE, rightE, caseE]) ?? leftE ?? rightE ?? caseE ?? 0);
        const sessionDrain = roundToStep(Math.max(0, sessionStart - sessionEnd));
        categories = ['Start', 'End', 'Drain'];
        series = [
          {
            name: 'Session',
            data: [sessionStart ?? 0, sessionEnd ?? 0, sessionDrain ?? 0]
          }
        ];
        colors = [greenColor];
      } else {
        const radarRows = rawData.slice(-6);
        const radarCategories = radarRows.map(pt => pt.label);
        const leftSeries = radarRows.map((_, idx) => leftLevels[leftLevels.length - radarRows.length + idx] ?? 0);
        const rightSeries = radarRows.map((_, idx) => rightLevels[rightLevels.length - radarRows.length + idx] ?? 0);
        const caseSeries = radarRows.map((_, idx) => caseLevels[caseLevels.length - radarRows.length + idx] ?? 0);
        categories = radarCategories;
        series = [
          { name: 'Left Bud', data: leftSeries },
          { name: 'Right Bud', data: rightSeries },
          { name: 'Case', data: caseSeries }
        ];
        colors = [greenColor, blueColor, purpleColor];
      }

      options.tooltip = {
        theme: 'dark',
        shared: false,
        intersect: true,
        custom: function ({ seriesIndex, dataPointIndex, w }) {
          const rawValue = w.globals.series?.[seriesIndex]?.[dataPointIndex] ?? 0;
          const label = w.globals.labels?.[dataPointIndex] || categories[dataPointIndex] || 'Radar summary';
          return buildRadarBucketTooltipHtml({
            durationLabel: radarDurationLabel,
            itemLabel: radarItemLabel,
            bucketLabel: label,
            seriesLabel: w.globals.seriesNames?.[seriesIndex] || series?.[seriesIndex]?.name || radarItemLabel,
            value: rawValue,
            rowCount: rawData.length,
            left: leftLevels[dataPointIndex] ?? 0,
            right: rightLevels[dataPointIndex] ?? 0,
            case: caseLevels[dataPointIndex] ?? 0,
          });
        }
      };
    } else {
      // Line, Area, Bar
      if (item === 'left') {

        series = [{ name: 'Left Bud', data: leftLevels }];
        colors = [greenColor];
      } else if (item === 'right') {
        series = [{ name: 'Right Bud', data: rightLevels }];
        colors = [blueColor];
      } else if (item === 'case') {
        series = [{ name: 'Case', data: caseLevels }];
        colors = [purpleColor];
      } else if (item === 'avg') {
        series = [{ name: 'Average Bud', data: avgLevels }];
        colors = [whiteColor];
      } else {
        series = [
          { name: 'Left Bud', data: leftLevels },
          { name: 'Right Bud', data: rightLevels },
          { name: 'Case', data: caseLevels }
        ];
        colors = [greenColor, blueColor, purpleColor];
      }
    }
  }

  const isPieOrDonut = chartType === 'pie' || chartType === 'donut';

  const options = {
    chart: {
      type: chartType,
      height: chartType === 'pie' || chartType === 'donut' ? 380 : 320,
      background: 'transparent',
      fontFamily: '"Segoe UI Variable", "Segoe UI", system-ui, sans-serif',
      toolbar: { show: false },
      animations: {
        enabled: true,
        easing: 'easeinout',
        speed: 800,
        dynamicAnimation: { enabled: true, speed: 350 }
      }
    },
    colors: colors,
    theme: { mode: 'dark' },
    dataLabels: {
      enabled: false
    },
    stroke: {
      show: true,
      curve: 'smooth',
      width: isPieOrDonut ? 0 : (chartType === 'bar' ? 0 : (chartType === 'area' ? 3 : 2))
    },
    fill: chartType === 'area'
      ? {
        type: 'gradient',
        opacity: 0.9,
        gradient: {
          shadeIntensity: 0.35,
          opacityFrom: 0.42,
          opacityTo: 0.08,
          stops: [0, 70, 100]
        }
      }
      : {
        type: 'solid',
        opacity: 1
      },
    grid: {
      borderColor: 'rgba(255, 255, 255, 0.08)',
      strokeDashArray: 4,
      xaxis: { lines: { show: false } },
      yaxis: { lines: { show: true } },
      padding: { top: 10, right: 20, bottom: 0, left: 10 }
    },
    markers: {
      size: duration === 'session' ? 6 : (chartType === 'line' ? 4 : (chartType === 'area' ? 2 : 0)),
      colors: colors,
      strokeColors: '#111113',
      strokeWidth: 2,
      hover: { size: 6 }
    },
    tooltip: {
      theme: 'dark',
      x: { show: true },
      y: {
        formatter: function (val) {
          return val + '%';
        }
      }
    },
    legend: {
      show: isPieOrDonut || chartType === 'radar' || item === 'all',
      position: 'top',
      horizontalAlign: 'right',
      fontSize: '12px',
      markers: { radius: 12 },
      labels: { colors: '#888898' }
    }
  };

  if (isPieOrDonut) {
    options.series = series;
    options.labels = categories;
    options.stroke = { show: false };
    options.dataLabels = {
      enabled: true,
      formatter: function (val, opts) {
        return opts.w.config.series[opts.seriesIndex] + '%';
      }
    };
    options.plotOptions = {
      pie: {
        expandOnClick: true,
        customScale: 0.92,
        donut: {
          size: '70%',
          labels: {
            show: true,
            name: { show: true, color: '#888898' },
            value: {
              show: true,
              color: '#ffffff',
              formatter: function (val) { return val + '%'; }
            },
            total: {
              show: true,
              label: duration === 'session' ? 'Total Drain' : 'Avg Level',
              color: '#888898',
              formatter: function (w) {
                if (duration === 'session') {
                  return w.globals.seriesTotals.reduce((a, b) => a + b, 0) + '%';
                }
                const totals = w.globals.seriesTotals.filter(v => v != null);
                if (!totals.length) return '0%';
                return `${Math.round(totals.reduce((a, b) => a + b, 0) / totals.length)}%`;
              }
            }
          }
        }
      }
    };
    options.legend = {
      show: true,
      position: 'bottom',
      horizontalAlign: 'center',
      fontSize: '12px',
      markers: { radius: 12 },
      labels: { colors: '#888898' }
    };
  } else if (chartType === 'radar') {
    options.legend = {
      show: true,
      position: 'top',
      horizontalAlign: 'center',
      fontSize: '12px',
      markers: { radius: 12 },
      labels: { colors: '#888898' }
    };
  } else {
    options.legend = {
      show: item === 'all',
      position: 'top',
      horizontalAlign: 'right',
      fontSize: '12px',
      markers: { radius: 12 },
      labels: { colors: '#888898' }
    };
    options.series = series;
    options.xaxis = {
      categories: categories,
      axisBorder: { show: false },
      axisTicks: { show: false },
      tickAmount: categories.length > 10 ? 10 : undefined,
      labels: {
        style: { colors: '#888898', fontSize: '11px' },
        hideOverlappingLabels: true,
        trim: true
      }
    };

    // Calculate dynamic axis range and clean ticks based on data and batteryStep
    let yMin = 0;
    let yMax = undefined;
    let tickAmount = undefined;

    let allVals = [];
    series.forEach(s => {
      s.data.forEach(val => {
        if (val != null) allVals.push(val);
      });
    });

    if (allVals.length > 0) {
      let minVal = 0;
      let maxVal = 100;

      if (duration === 'session') {
        // Always show full range (0 - 100%) for battery levels, with clean 20% intervals (0%, 20%, 40%, 60%, 80%, 100%)
        yMin = 0;
        yMax = 100;
        tickAmount = 5;
      } else {
        // Delta drain range (0 to Max + 2 steps headroom)
        yMin = 0;
        const rawTop = Math.ceil(maxVal / batteryStep) * batteryStep;
        // Add 2 extra step-intervals above the max so the chart isn't cramped
        yMax = rawTop + 2 * batteryStep;
        // Auto-merge intervals: if > 8 ticks, double the effective step until we're at ≤8
        let effectiveStep = batteryStep;
        while ((yMax / effectiveStep) > 8) {
          effectiveStep *= 2;
          // Re-snap yMax to the larger step
          yMax = Math.ceil((rawTop + 2 * batteryStep) / effectiveStep) * effectiveStep;
        }
        tickAmount = Math.round(yMax / effectiveStep);
      }
    }

    if (duration !== 'session') {
      yMin = 0;
      yMax = 100;
      tickAmount = 5;
    }

    options.yaxis = {
      min: yMin,
      max: yMax,
      tickAmount: tickAmount,
      labels: {
        style: { colors: '#888898', fontSize: '11px' },
        formatter: function (val) {
          return Math.round(val) + '%';
        }
      }
    };
  }

  if (isPieOrDonut && series.every(v => v == null || Number.isNaN(v))) {
    canvasEl.style.display = 'none';
    if (emptyEl) {
      emptyEl.style.display = 'flex';
      emptyEl.innerHTML = `
        <svg width="48" height="48" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" style="opacity: 0.4;">
          <path d="M3 3v18h18"/>
          <path d="M18.7 8l-5.1 5.2-2.8-2.7L7 14.3"/>
        </svg>
        <p>No drain data found for the selected range.</p>
      `;
    }
    if (batteryChart) {
      batteryChart.destroy();
      batteryChart = null;
    }
    return;
  }

  if (chartType === 'bar') {
    options.plotOptions = {
      bar: {
        borderRadius: 4,
        columnWidth: '45%'
      }
    };
  }

  if ((chartType === 'pie' || chartType === 'donut') && item !== 'all') {
    canvasEl.style.display = 'none';
    if (emptyEl) {
      emptyEl.style.display = 'flex';
      emptyEl.innerHTML = `
        <svg width="48" height="48" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" style="opacity: 0.4;">
          <path d="M3 3v18h18"/>
          <path d="M18.7 8l-5.1 5.2-2.8-2.7L7 14.3"/>
        </svg>
        <p>Pie and donut charts are only available when Item is set to All.</p>
      `;
    }
    if (batteryChart) {
      batteryChart.destroy();
      batteryChart = null;
    }
    return;
  }

  if (chartType === 'area') {
    options.stroke.width = 3;
  }

  if (chartType === 'radar') {
    // ApexCharts radar has different layout behavior depending on whether
    // the x-axis categories are numeric/labels. For “any duration”, ensure:
    // - stable category count/order
    // - stable 0..100 scale
    // - consistent polygon fill/stroke rendering
    const radarCategories = (Array.isArray(categories) && categories.length)
      ? [...categories]
      : ['Left Bud', 'Right Bud', 'Case'];

    const radarSeries = JSON.parse(JSON.stringify(series));

    options.chart.type = 'radar';
    // Make tiny drains visible: plot with a small visual epsilon lift,
    // but keep tooltip values as the real drains.
    const MIN_VISIBLE_EPS = Math.max(0.8, (batteryStep && Number(batteryStep) > 0 ? Number(batteryStep) : 5) * 0.2);
    const liftVisual = (v) => {
      const num = Number(v);
      if (!Number.isFinite(num) || num <= 0) return 0;
      return Math.max(num, MIN_VISIBLE_EPS);
    };

    const radarSeriesVisual = JSON.parse(JSON.stringify(radarSeries));
    radarSeriesVisual.forEach(s => {
      if (Array.isArray(s.data)) s.data = s.data.map(liftVisual);
    });

    options.series = radarSeriesVisual;

    // Force radar-specific x/y axis settings for consistent UI
    options.xaxis = {
      categories: radarCategories,
      labels: {
        show: true,
        style: { colors: '#888898', fontSize: '11px' },
        trim: true
      }
    };

    options.yaxis = {
      show: false,
      min: 0,
      max: 100,
      tickAmount: 5
    };

    options.grid = {
      show: false
    };

    options.stroke = {
      show: true,
      curve: 'smooth',
      width: 3
    };

    options.fill = {
      type: 'solid',
      opacity: 0.18
    };

    options.markers = {
      size: 5,
      colors: colors,
      strokeColors: '#111113',
      strokeWidth: 2,
      hover: { size: 7 }
    };

    options.legend = {
      show: true,
      position: 'top',
      horizontalAlign: 'center',
      fontSize: '12px',
      markers: { radius: 12 },
      labels: { colors: '#888898' }
    };

    // Ensure radar polygons render consistently regardless of duration.
    options.plotOptions = {
      radar: {
        polygons: {
          strokeColors: 'rgba(255,255,255,0.08)',
          connectorColors: 'rgba(255,255,255,0.08)'
        }
      }
    };
  }

  if (batteryChart) {
    batteryChart.destroy();
    batteryChart = null;
  }

  const freshCanvasEl = canvasEl.cloneNode(false);
  canvasEl.replaceWith(freshCanvasEl);
  if (loadToken !== batteryGraphLoadToken) return;
  batteryChart = new ApexCharts(freshCanvasEl, options);
  requestAnimationFrame(() => {
    if (loadToken !== batteryGraphLoadToken) return;
    if (batteryChart) {
      batteryChart.render().catch(console.error);
    }
  });
}

// Wire up dropdown events
document.getElementById('graph-duration')?.addEventListener('change', loadBatteryGraph);
document.getElementById('graph-item')?.addEventListener('change', loadBatteryGraph);
document.getElementById('graph-type')?.addEventListener('change', loadBatteryGraph);
document.getElementById('stats-week-prev')?.addEventListener('click', () => changeStatsWeek(1));
document.getElementById('stats-week-next')?.addEventListener('click', () => changeStatsWeek(-1));

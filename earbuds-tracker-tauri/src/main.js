// main.js – EarbudsTracker frontend logic
// Tauri v2 exposes invoke under window.__TAURI__.core, not window.__TAURI__ directly
const invoke = window.__TAURI__.core.invoke;
const event  = window.__TAURI__.event;

// ── Preferences State ──────────────────────────────────────────────────────────
let currentGoal = parseFloat(localStorage.getItem('playback-goal') || '2.0');
let notificationsEnabled = localStorage.getItem('notifications-enabled') !== 'false';
let currentDeviceName = localStorage.getItem('target-device') || 'CMF Buds 2a';

// Send initial device name to backend
invoke('set_device_name', { name: currentDeviceName }).catch(console.error);

// State trackers for notifications & animations
let wasConnected = null;
let lastSessConn = 0;
let lastSessPlay = 0;
let lastConnected = false;
let lastPlaying = false;
let lastGoalMet = false;
let pulseFactor = 0;
let animationFrameId = null;
let mousePos = null;

// Elements
const goalInput = document.getElementById('goal-input');
const notificationToggle = document.getElementById('notification-toggle');
const deviceNameInput = document.getElementById('device-name-input');

// Initialize settings inputs
if (goalInput) {
  goalInput.value = currentGoal;
  goalInput.addEventListener('change', (e) => {
    currentGoal = parseFloat(e.target.value) || 2.0;
    localStorage.setItem('playback-goal', currentGoal);
    refreshSnapshot();
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
    select.value = val.toString();
  } catch (e) {
    console.error('get_battery_interval failed', e);
  }

  select.addEventListener('change', async (e) => {
    const secs = parseInt(e.target.value);
    if (!isNaN(secs)) {
      try {
        await invoke('set_battery_interval', { secs });
      } catch (err) {
        console.error('set_battery_interval failed', err);
      }
    }
  });
}

initBatteryIntervalSelect();

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
  return `${String(h).padStart(2,'0')}:${String(m).padStart(2,'0')}:${String(sc).padStart(2,'0')}`;
}

function fmtH(secs) {
  const s = Math.max(0, Math.floor(secs));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sc = s % 60;
  if (h) return `${h}h ${String(m).padStart(2,'0')}m`;
  if (m) return `${m}m ${String(sc).padStart(2,'0')}s`;
  return `${sc}s`;
}

// ── Ring canvas ───────────────────────────────────────────────────────────────
const canvas = document.getElementById('ring-canvas');
const ctx    = canvas.getContext('2d');
const dpr    = window.devicePixelRatio || 1;
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
    const rIn  = rOut - 18;
    const dx = mx - cx;
    const dy = my - cy;
    const dist = Math.sqrt(dx*dx + dy*dy);
    
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
    const rIn  = rOut - 18;
    const dx = mx - cx;
    const dy = my - cy;
    const dist = Math.sqrt(dx*dx + dy*dy);
    
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
  const rIn  = rOut - 18;
  const sw   = 10;

  const trackOut = playing ? '#2d2d2d' : '#2a2a2e';
  const trackIn  = playing ? '#1f3a22' : '#2e2e34';
  const connCol  = connected ? '#ffffff' : '#3a3a40';
  const playCol  = playing   
    ? (goalMet ? '#fbbf24' : '#4ade80') 
    : '#2a3a2a';

  // Apply smooth pulse glow to playback ring when active
  if (playing) {
    ctx.shadowBlur = (goalMet ? 6 : 4) * pulseFactor;
    ctx.shadowColor = goalMet ? '#fbbf24' : '#4ade80';
  } else {
    ctx.shadowBlur = 0;
  }

  function arc(r, start, span, color) {
    ctx.beginPath();
    ctx.arc(cx, cy, r, start, start + span, span < 0);
    ctx.strokeStyle = color;
    ctx.lineWidth   = sw;
    ctx.lineCap     = 'round';
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
  arc(rIn,  0, Math.PI * 2, trackIn);

  // Inner progress arc (playback) with glow
  if (playing) {
    ctx.shadowBlur = (goalMet ? 6 : 4) * pulseFactor;
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
    const dist = Math.sqrt(dx*dx + dy*dy);
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
    ctx.shadowBlur = 6;
    ctx.shadowColor = goalMet ? 'rgba(251,191,36,0.25)' : 'rgba(74,222,128,0.25)';
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
        const opacity = 0.12 * (1 - scale);
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

  const totalTodayPlay = today.playback;
  const goalMet = totalTodayPlay >= (currentGoal * 3600);

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

  // Status card
  const dot   = document.getElementById('status-dot');
  const title = document.getElementById('status-title');
  const desc  = document.getElementById('status-desc');
  if (playing) {
    if (dot) dot.className = 'status-dot playing';
    title.textContent = goalMet ? 'Playing (Goal Met!)' : 'Playing';
    title.style.color = goalMet ? '#fbbf24' : 'var(--green)';
    desc.textContent  = 'Streaming live audio';
  } else if (connected) {
    if (dot) dot.className = 'status-dot connected';
    title.textContent = 'Connected (Idle)';
    title.style.color = '#d4a017';
    desc.textContent  = 'Link active – no media playing';
  } else {
    if (dot) dot.className = 'status-dot disconnected';
    title.textContent = 'Disconnected';
    title.style.color = 'var(--muted)';
    desc.textContent  = `${currentDeviceName} is out of range or off`;
  }

  // Stats page
  document.getElementById('s-today-conn').textContent  = fmtH(today.connected);
  document.getElementById('s-today-play').textContent  = fmtH(today.playback);
  document.getElementById('s-week-conn').textContent   = fmtH(week.connected);
  document.getElementById('s-week-play').textContent   = fmtH(week.playback);
  document.getElementById('s-month-conn').textContent  = fmtH(month.connected);
  document.getElementById('s-month-play').textContent  = fmtH(month.playback);
  document.getElementById('s-life-conn').textContent   = fmtH(lifetime.connected);
  document.getElementById('s-life-play').textContent   = fmtH(lifetime.playback);

  // If stats page is active, update chart
  const activePage = document.querySelector('.nav-item.active').dataset.page;
  if (activePage === 'statistics') {
    updateDailyStatsAndChart();
  }
}

// ── Daily Stats & Chart ───────────────────────────────────────────────────────
async function updateDailyStatsAndChart() {
  let history = [];
  try {
    history = await invoke('get_daily_history');
  } catch (e) {
    console.error('get_daily_history failed', e);
  }
  calculateStreak(history);
  drawDailyChart(history);
}

function calculateStreak(history) {
  if (!history || history.length === 0) {
    document.getElementById('streak-badge-container').style.display = 'none';
    return;
  }

  const playbackMap = new Map();
  history.forEach(row => {
    playbackMap.set(row.day, row.connected_secs > 0 ? row.playback_secs : 0);
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

  let streak = 0;
  let checkDate = new Date();

  const todayPlay = playbackMap.get(todayStr) || 0;
  const yestPlay = playbackMap.get(yesterdayStr) || 0;

  if (todayPlay > 0) {
    streak = 1;
    checkDate.setDate(today.getDate() - 1);
  } else if (yestPlay > 0) {
    streak = 1;
    checkDate.setDate(yesterday.getDate() - 1);
  } else {
    document.getElementById('streak-badge-container').style.display = 'none';
    return;
  }

  while (true) {
    const dateStr = getLocalDateString(checkDate);
    const playTime = playbackMap.get(dateStr) || 0;
    if (playTime > 0) {
      streak++;
      checkDate.setDate(checkDate.getDate() - 1);
    } else {
      break;
    }
  }

  if (streak > 0) {
    document.getElementById('streak-count').textContent = streak;
    document.getElementById('streak-badge-container').style.display = 'inline-flex';
  } else {
    document.getElementById('streak-badge-container').style.display = 'none';
  }
}

function drawDailyChart(history) {
  const chartCanvas = document.getElementById('stats-chart');
  if (!chartCanvas) return;

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

  const days = [];
  const getLocalDateString = (d) => {
    const y = d.getFullYear();
    const m = String(d.getMonth() + 1).padStart(2, '0');
    const day = String(d.getDate()).padStart(2, '0');
    return `${y}-${m}-${day}`;
  };

  const getDayLabel = (d) => {
    const weekdays = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'];
    return weekdays[d.getDay()] + ' ' + d.getDate();
  };

  for (let i = 6; i >= 0; i--) {
    let d = new Date();
    d.setDate(d.getDate() - i);
    days.push({
      dateStr: getLocalDateString(d),
      label: getDayLabel(d),
      connected: 0,
      playback: 0
    });
  }

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
  const gap = (w - (barWidth * 7)) / 8;

  c.strokeStyle = '#2a2a2f';
  c.lineWidth = 1;
  c.font = '10px "Segoe UI", sans-serif';
  c.fillStyle = '#888898';
  
  for (let i = 0; i <= 2; i++) {
    const y = chartTop + (chartHeight * i / 2);
    c.beginPath();
    c.moveTo(gap, y);
    c.lineTo(w - gap, y);
    c.stroke();
    
    const labelVal = maxSecs - (maxSecs * i / 2);
    c.fillText(fmtH(labelVal), 10, y + 4);
  }

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

    c.fillStyle = '#888898';
    c.font = '11px "Segoe UI", sans-serif';
    c.textAlign = 'center';
    c.fillText(day.label, x + barWidth / 2, chartBottom + 18);
  });
}

function updateBatteryUI(batteryInfo) {
  const card = document.getElementById('battery-card');
  if (!card) return;

  // Always show the battery card on the dashboard
  card.style.display = 'block';

  let isLive = false;
  if (batteryInfo) {
    isLive = true;
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

  const formatBud = (conn, disc) => {
    if (conn === null && disc === null) return '—';
    const connStr = conn !== null ? `${conn}%` : '—';
    const discStr = disc !== null ? `${disc}%` : '—';
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
    const start = r.session_start.replace('T', '  ').slice(0, 21);
    const end   = r.session_end ? r.session_end.replace('T', '  ').slice(0, 21) : '—';
    tr.innerHTML = `
      <td>${start}</td>
      <td>${end}</td>
      <td>${fmtH(r.connected_secs)}</td>
      <td style="color:var(--green)">${fmtH(r.playback_secs)}</td>
      <td>${fmtSessBattery(r)}</td>
    `;
    tbody.appendChild(tr);
  });
}

document.getElementById('refresh-btn').addEventListener('click', loadHistory);

// ── Settings – reset (2-step: confirm → Windows auth) ────────────────────────
const dialog     = document.getElementById('dialog');
const authDialog = document.getElementById('auth-dialog');
const authPwdInput = document.getElementById('auth-password-input');
const authError    = document.getElementById('auth-error');
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
  if (authError)    authError.style.display = 'none';
  authDialog.hidden = false;
  // Auto-focus the password field
  setTimeout(() => authPwdInput && authPwdInput.focus(), 60);
});

// Step 2 – cancel
document.getElementById('auth-cancel').addEventListener('click', () => {
  authDialog.hidden = true;
  if (authPwdInput) authPwdInput.value = '';
  if (authError)    authError.style.display = 'none';
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
      await refreshSnapshot();
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
let bdSelectedId  = null;    // currently selected session id
let bdNoteTimer   = null;    // debounce timer for note autosave

// ── Format helpers ────────────────────────────────────────────────────────────
function bdFmtDur(secs) {
  if (!secs || secs <= 0) return '—';
  return fmtH(secs);
}

function bdFmtDate(ts) {
  if (!ts) return '—';
  return ts.replace('T', '\u00A0\u00A0').slice(0, 21);
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
  } catch(e) {
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
  const q    = (document.getElementById('bd-search')?.value || '').toLowerCase();
  const sort = document.getElementById('bd-sort')?.value || 'newest';

  let filtered = bdAllSessions.filter(s => {
    if (q) {
      if (!s.session_start.toLowerCase().includes(q)) return false;
    }
    return true;
  });

  filtered.sort((a, b) => {
    if (sort === 'oldest')    return a.session_start.localeCompare(b.session_start);
    if (sort === 'time-high') return b.connected_secs - a.connected_secs;
    if (sort === 'time-low')  return a.connected_secs - b.connected_secs;
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
    opt.textContent = `${date}\u00A0\u00A0│\u00A0\u00A0⇄\u00A0Conn:\u00A0${conn}\u00A0\u00A0│\u00A0\u00A0▶\u00A0Play:\u00A0${play}`;
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
  } catch(e) {
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
  const batL0 = s.bat_left_connect,   batL1 = s.bat_left_disc;
  const batR0 = s.bat_right_connect,  batR1 = s.bat_right_disc;
  const batC0 = s.bat_case_connect,   batC1 = s.bat_case_disc;

  const drainStr = (a, b) => {
    if (a == null || b == null) return '—';
    const d = a - b;
    return `${a}% → ${b}% (${d >= 0 ? '-' : '+'}${Math.abs(d)}%)`;
  };

  // Most used app
  const topApp = bd.app_totals.length > 0 ? bd.app_totals[0] : null;

  container.innerHTML = `
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
    <div class="card" id="bd-bat-card" style="${(batL0 == null && batR0 == null) ? 'display:none' : ''}">
      <div class="bd-section-title">Battery Drain Curve</div>
      <div class="bd-battery-canvas-wrap">
        <canvas id="bd-bat-canvas"></canvas>
      </div>
    </div>

  `;

  // Render app bars
  bdRenderAppBars(bd.app_totals, s.playback_secs);

  // Render battery drain canvas
  if (batL0 != null || batR0 != null) {
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
function bdDrawBatteryCurve(s) {
  const canvas = document.getElementById('bd-bat-canvas');
  if (!canvas) return;
  const wrap = canvas.parentElement;
  const dpr = window.devicePixelRatio || 1;
  const W = wrap.clientWidth || 400;
  const H = 140;
  canvas.width  = W * dpr;
  canvas.height = H * dpr;
  canvas.style.width  = W + 'px';
  canvas.style.height = H + 'px';

  const c = canvas.getContext('2d');
  c.scale(dpr, dpr);
  c.clearRect(0, 0, W, H);

  const padL = 38, padR = 16, padT = 12, padB = 28;
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
    c.fillText(pct + '%', padL - 4, y + 3);
  });

  // X labels
  c.textAlign = 'center';
  c.fillStyle = '#888898';
  c.font = '10px "Segoe UI", sans-serif';
  c.fillText('Start', padL, H - 6);
  c.fillText('End', padL + chartW, H - 6);

  // Lines per earbud
  const series = [
    { label: 'L', v0: s.bat_left_connect,  v1: s.bat_left_disc,  color: '#6366f1' },
    { label: 'R', v0: s.bat_right_connect, v1: s.bat_right_disc, color: '#4ade80' },
    { label: 'C', v0: s.bat_case_connect,  v1: s.bat_case_disc,  color: '#fbbf24' },
  ].filter(sr => sr.v0 != null || sr.v1 != null);

  series.forEach(sr => {
    const x0 = padL, x1 = padL + chartW;
    const start = sr.v0 ?? sr.v1 ?? 0;
    const end   = sr.v1 ?? sr.v0 ?? 0;
    const y0 = padT + chartH - (start / 100) * chartH;
    const y1 = padT + chartH - (end   / 100) * chartH;

    // Gradient fill
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

    // Line
    c.beginPath();
    c.moveTo(x0, y0);
    c.lineTo(x1, y1);
    c.strokeStyle = sr.color;
    c.lineWidth = 2;
    c.stroke();

    // Dots
    [{ x: x0, y: y0, v: start }, { x: x1, y: y1, v: end }].forEach(pt => {
      c.beginPath();
      c.arc(pt.x, pt.y, 4, 0, Math.PI * 2);
      c.fillStyle = sr.color;
      c.fill();
      c.fillStyle = sr.color;
      c.font = 'bold 9px "Segoe UI", sans-serif';
      c.textAlign = 'center';
      c.fillText(sr.label + ':' + pt.v + '%', pt.x, pt.y - 8);
    });
  });
}

// ── Export session data ───────────────────────────────────────────────────────
async function bdExport(sessionId, format) {
  let content;
  try {
    content = await invoke('export_session', { sessionId, format });
  } catch(e) {
    console.error('export_session failed', e);
    return;
  }
  if (!content) return;
  const mime = format === 'csv' ? 'text/csv' : 'application/json';
  const ext  = format === 'csv' ? 'csv' : 'json';
  const blob = new Blob([content], { type: mime });
  const url  = URL.createObjectURL(blob);
  const a    = document.createElement('a');
  a.href     = url;
  a.download = `session_${sessionId}.${ext}`;
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}

// ── Wire up filters & session picker ─────────────────────────────────────────
document.getElementById('bd-search')?.addEventListener('input',  bdRenderList);
document.getElementById('bd-sort')?.addEventListener('change',   bdRenderList);
document.getElementById('bd-session-picker')?.addEventListener('change', e => {
  const id = parseInt(e.target.value);
  if (!isNaN(id)) bdSelectSession(id);
});

// ── Load breakdown data whenever the nav item is clicked ──────────────────────
document.querySelector('.nav-item[data-page="breakdown"]')
  ?.addEventListener('click', bdLoadSessions);

// ── Battery Analytics Graph ──────────────────────────────────────────────────
let batteryChart = null;

async function loadBatteryGraph() {
  const canvasEl = document.getElementById('battery-graph-canvas');
  const emptyEl = document.getElementById('graph-empty-state');
  if (!canvasEl) return;

  const duration = document.getElementById('graph-duration')?.value || 'week';
  const item = document.getElementById('graph-item')?.value || 'all';
  let chartType = document.getElementById('graph-type')?.value || 'line';

  // Dropdown dependency logic: disable Pie/Donut for single items
  const typeSelect = document.getElementById('graph-type');
  if (typeSelect) {
    const pieOpt = typeSelect.querySelector('option[value="pie"]');
    const donutOpt = typeSelect.querySelector('option[value="donut"]');
    if (item !== 'all') {
      if (pieOpt) pieOpt.disabled = true;
      if (donutOpt) donutOpt.disabled = true;
      if (chartType === 'pie' || chartType === 'donut') {
        chartType = 'line';
        typeSelect.value = 'line';
      }
    } else {
      if (pieOpt) pieOpt.disabled = false;
      if (donutOpt) donutOpt.disabled = false;
    }
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
      const statusEl = document.getElementById('battery-status-sub');
      isLive = statusEl && statusEl.textContent.includes('Connected');
    } catch(e) {}
  }

  // Fetch data
  let rawData = [];
  try {
    rawData = await invoke('get_battery_graph_data', { duration });
  } catch(e) {
    console.error('get_battery_graph_data failed', e);
  }

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

  let categories = [];
  let series = [];

  const greenColor = '#4ade80';
  const blueColor = '#60a5fa';
  const purpleColor = '#a78bfa';
  const whiteColor = '#ffffff';

  let colors = [];

  if (duration === 'session') {
    const pt = rawData[0];
    const leftE = pt.left_end ?? (isLive ? liveLeft : pt.left_start);
    const rightE = pt.right_end ?? (isLive ? liveRight : pt.right_start);
    const caseE = pt.case_end ?? (isLive ? liveCase : pt.case_start);

    categories = ['Start Connection', 'Current / Disconnect'];

    if (item === 'left') {
      series = [{ name: 'Left Bud', data: [pt.left_start ?? 0, leftE ?? 0] }];
      colors = [greenColor];
    } else if (item === 'right') {
      series = [{ name: 'Right Bud', data: [pt.right_start ?? 0, rightE ?? 0] }];
      colors = [blueColor];
    } else if (item === 'case') {
      series = [{ name: 'Case', data: [pt.case_start ?? 0, caseE ?? 0] }];
      colors = [purpleColor];
    } else if (item === 'avg') {
      const startAvg = ((pt.left_start ?? 0) + (pt.right_start ?? 0)) / 2;
      const endAvg = ((leftE ?? 0) + (rightE ?? 0)) / 2;
      series = [{ name: 'Average Bud', data: [startAvg, endAvg] }];
      colors = [whiteColor];
    } else { // all
      series = [
        { name: 'Left Bud', data: [pt.left_start ?? 0, leftE ?? 0] },
        { name: 'Right Bud', data: [pt.right_start ?? 0, rightE ?? 0] },
        { name: 'Case', data: [pt.case_start ?? 0, caseE ?? 0] }
      ];
      colors = [greenColor, blueColor, purpleColor];
    }
  } else {
    // Aggregated session/day/week/month drain delta
    categories = rawData.map(pt => pt.label);

    const getDrain = (start, end, liveVal) => {
      const s = start;
      const e = end ?? (isLive ? liveVal : start);
      if (s === null || e === null) return 0;
      return Math.max(0, s - e);
    };

    const leftDrains = rawData.map(pt => getDrain(pt.left_start, pt.left_end, liveLeft));
    const rightDrains = rawData.map(pt => getDrain(pt.right_start, pt.right_end, liveRight));
    const caseDrains = rawData.map(pt => getDrain(pt.case_start, pt.case_end, liveCase));
    const avgDrains = rawData.map((pt, idx) => (leftDrains[idx] + rightDrains[idx]) / 2);

    if (chartType === 'pie' || chartType === 'donut') {
      const totalLeft = leftDrains.reduce((a, b) => a + b, 0);
      const totalRight = rightDrains.reduce((a, b) => a + b, 0);
      const totalCase = caseDrains.reduce((a, b) => a + b, 0);

      series = [totalLeft, totalRight, totalCase];
      categories = ['Left Bud', 'Right Bud', 'Case'];
      colors = [greenColor, blueColor, purpleColor];
    } else if (chartType === 'radar') {
      if (item === 'all') {
        const maxLeft = Math.max(...leftDrains, 0);
        const maxRight = Math.max(...rightDrains, 0);
        const maxCase = Math.max(...caseDrains, 0);

        const avgLeft = leftDrains.reduce((a,b)=>a+b,0) / (leftDrains.length || 1);
        const avgRight = rightDrains.reduce((a,b)=>a+b,0) / (rightDrains.length || 1);
        const avgCase = caseDrains.reduce((a,b)=>a+b,0) / (caseDrains.length || 1);

        series = [
          { name: 'Max Drain', data: [maxLeft, maxRight, maxCase] },
          { name: 'Average Drain', data: [parseFloat(avgLeft.toFixed(1)), parseFloat(avgRight.toFixed(1)), parseFloat(avgCase.toFixed(1))] }
        ];
        categories = ['Left Bud', 'Right Bud', 'Case'];
        colors = [greenColor, whiteColor];
      } else {
        let targetData = [];
        let name = '';
        if (item === 'left') { targetData = leftDrains; name = 'Left Bud'; colors = [greenColor]; }
        else if (item === 'right') { targetData = rightDrains; name = 'Right Bud'; colors = [blueColor]; }
        else if (item === 'case') { targetData = caseDrains; name = 'Case'; colors = [purpleColor]; }
        else { targetData = avgDrains; name = 'Average'; colors = [whiteColor]; }

        series = [{ name, data: targetData }];
      }
    } else {
      // Line, Area, Bar
      if (item === 'left') {
        series = [{ name: 'Left Bud Drain', data: leftDrains }];
        colors = [greenColor];
      } else if (item === 'right') {
        series = [{ name: 'Right Bud Drain', data: rightDrains }];
        colors = [blueColor];
      } else if (item === 'case') {
        series = [{ name: 'Case Drain', data: caseDrains }];
        colors = [purpleColor];
      } else if (item === 'avg') {
        series = [{ name: 'Average Bud Drain', data: avgDrains }];
        colors = [whiteColor];
      } else { // all
        series = [
          { name: 'Left Bud Drain', data: leftDrains },
          { name: 'Right Bud Drain', data: rightDrains },
          { name: 'Case Drain', data: caseDrains }
        ];
        colors = [greenColor, blueColor, purpleColor];
      }
    }
  }

  const isPieOrDonut = chartType === 'pie' || chartType === 'donut';

  const options = {
    chart: {
      type: chartType,
      height: 320,
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
    stroke: {
      show: true,
      curve: 'smooth',
      width: isPieOrDonut ? 0 : (chartType === 'bar' ? 0 : 2)
    },
    fill: {
      type: chartType === 'area' ? 'gradient' : 'solid',
      gradient: {
        shadeIntensity: 1,
        opacityFrom: 0.4,
        opacityTo: 0.02,
        stops: [0, 95, 100]
      }
    },
    grid: {
      borderColor: 'rgba(255, 255, 255, 0.08)',
      strokeDashArray: 4,
      xaxis: { lines: { show: false } },
      yaxis: { lines: { show: true } },
      padding: { top: 10, right: 20, bottom: 0, left: 10 }
    },
    markers: {
      size: duration === 'session' ? 6 : (chartType === 'line' || chartType === 'area' ? 4 : 0),
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
      show: true,
      position: 'top',
      horizontalAlign: 'right',
      fontSize: '12px',
      markers: { radius: 12 },
      labels: { colors: '#888898' }
    }
  };

  if (isPieOrDonut) {
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
              label: 'Total Drain',
              color: '#888898',
              formatter: function (w) {
                return w.globals.seriesTotals.reduce((a, b) => a + b, 0) + '%';
              }
            }
          }
        }
      }
    };
  } else {
    options.series = series;
    options.xaxis = {
      categories: categories,
      axisBorder: { show: false },
      axisTicks: { show: false },
      labels: {
        style: { colors: '#888898', fontSize: '11px' }
      }
    };
    options.yaxis = {
      min: 0,
      max: duration === 'session' ? 100 : undefined,
      labels: {
        style: { colors: '#888898', fontSize: '11px' },
        formatter: function (val) {
          return Math.round(val) + '%';
        }
      }
    };
  }

  if (chartType === 'bar') {
    options.plotOptions = {
      bar: {
        borderRadius: 4,
        columnWidth: '45%'
      }
    };
  }

  if (chartType === 'radar') {
    options.yaxis = {
      show: false,
      min: 0
    };
    options.grid = { show: false };
  }

  if (batteryChart) {
    batteryChart.destroy();
  }

  batteryChart = new ApexCharts(canvasEl, options);
  batteryChart.render();
}

// Wire up dropdown events
document.getElementById('graph-duration')?.addEventListener('change', loadBatteryGraph);
document.getElementById('graph-item')?.addEventListener('change', loadBatteryGraph);
document.getElementById('graph-type')?.addEventListener('change', loadBatteryGraph);




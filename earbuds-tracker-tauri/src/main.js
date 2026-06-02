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
    ctx.shadowBlur = (goalMet ? 14 : 10) * pulseFactor;
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
  
  // Outer progress arc (connection)
  if (connSecs > 0) {
    const span = (Math.min(connSecs, MAX_SECS) / MAX_SECS) * Math.PI * 2;
    arc(rOut, -Math.PI / 2, span, connCol);
  }

  // Inner track
  ctx.shadowBlur = 0; // reset shadow for inner track
  arc(rIn,  0, Math.PI * 2, trackIn);

  // Inner progress arc (playback) with glow
  if (playing) {
    ctx.shadowBlur = (goalMet ? 14 : 10) * pulseFactor;
    ctx.shadowColor = goalMet ? '#fbbf24' : '#4ade80';
  }
  if (playSecs > 0) {
    const span = (Math.min(playSecs, MAX_SECS) / MAX_SECS) * Math.PI * 2;
    arc(rIn, -Math.PI / 2, span, playCol);
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
    ctx.shadowBlur = 12;
    ctx.shadowColor = 'rgba(255,255,255,0.3)';
    arc(rOut, -Math.PI / 2, connSecs > 0 ? (Math.min(connSecs, MAX_SECS) / MAX_SECS) * Math.PI * 2 : Math.PI * 2, connSecs > 0 ? '#ffffff' : '#555560');
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
    ctx.shadowBlur = 12;
    ctx.shadowColor = goalMet ? 'rgba(251,191,36,0.4)' : 'rgba(74,222,128,0.4)';
    arc(rIn, -Math.PI / 2, playSecs > 0 ? (Math.min(playSecs, MAX_SECS) / MAX_SECS) * Math.PI * 2 : Math.PI * 2, playSecs > 0 ? (goalMet ? '#fbbf24' : '#4ade80') : '#334433');
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

// ── History ───────────────────────────────────────────────────────────────────
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

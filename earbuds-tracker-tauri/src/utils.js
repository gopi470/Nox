/**
 * Core logic for calculating streaks, extracted for unit testing.
 * @param {Array} history - Array of { day: 'YYYY-MM-DD', playback_secs: number }
 * @param {number} goalHrs - Daily goal in hours
 * @param {number|null} todayPlaybackSecs - Optional current live playback seconds
 * @param {Date} now - Current date reference
 * @returns {Object} { streak: number, show: boolean }
 */
export function computeStreak(history, goalHrs, todayPlaybackSecs = null, now = new Date()) {
  if (!history || (history.length === 0 && (todayPlaybackSecs === null || todayPlaybackSecs === 0))) {
    return { streak: 0, show: false };
  }

  const goalSecs = goalHrs * 3600;
  const playbackMap = new Map();
  if (history) {
    history.forEach(row => {
      playbackMap.set(row.day, row.playback_secs || 0);
    });
  }

  const getLocalDateString = (d) => {
    const y = d.getFullYear();
    const m = String(d.getMonth() + 1).padStart(2, '0');
    const day = String(d.getDate()).padStart(2, '0');
    return `${y}-${m}-${day}`;
  };

  const today = new Date(now.getTime());
  const yesterday = new Date(now.getTime());
  yesterday.setDate(today.getDate() - 1);

  const todayStr = getLocalDateString(today);
  const yesterdayStr = getLocalDateString(yesterday);

  if (todayPlaybackSecs != null && Number.isFinite(todayPlaybackSecs)) {
    playbackMap.set(todayStr, Math.max(playbackMap.get(todayStr) || 0, todayPlaybackSecs));
  }

  let streak = 0;
  let checkDate = new Date(today.getTime());

  const todayPlay = playbackMap.get(todayStr) || 0;
  const yestPlay = playbackMap.get(yesterdayStr) || 0;

  if (todayPlay >= goalSecs) {
    streak = 1;
    checkDate.setDate(today.getDate() - 1);
  } else if (yestPlay >= goalSecs) {
    streak = 1;
    checkDate.setDate(yesterday.getDate() - 1);
  } else {
    return { streak: 0, show: false };
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

  return { streak, show: streak > 0 };
}

/**
 * Formats the time elapsed since the last battery update.
 * @param {number} ms - Timestamp in milliseconds
 * @param {number} now - Current timestamp in milliseconds
 * @param {number} batteryPollIntervalSec - Polling interval in seconds
 * @returns {string} Formatted string
 */
export function formatBatteryAgo(ms, now = Date.now(), batteryPollIntervalSec = 10) {
  if (!ms) return '0 sec ago';

  const seconds = Math.max(0, Math.floor((now - ms) / 1000));
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

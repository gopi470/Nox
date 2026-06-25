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
  if (goalSecs <= 0) {
    return { streak: 0, show: false };
  }
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

  if (todayPlaybackSecs != null && !Number.isNaN(todayPlaybackSecs)) {
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

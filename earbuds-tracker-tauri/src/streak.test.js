import { expect, test, describe } from 'vitest';
import { computeStreak } from './utils.js';

describe('computeStreak', () => {
  const goalHrs = 2;
  const goalSecs = goalHrs * 3600;
  const now = new Date('2023-10-10T12:00:00'); // Tuesday

  test('empty history and no today playback returns 0 streak', () => {
    const result = computeStreak([], goalHrs, 0, now);
    expect(result).toEqual({ streak: 0, show: false });
  });

  test('null history returns 0 streak', () => {
    const result = computeStreak(null, goalHrs, 0, now);
    expect(result).toEqual({ streak: 0, show: false });
  });

  test('streak of 1 if only today goal met', () => {
    const history = [];
    const todayPlayback = goalSecs + 1;
    const result = computeStreak(history, goalHrs, todayPlayback, now);
    expect(result).toEqual({ streak: 1, show: true });
  });

  test('streak of 1 if only yesterday goal met (today not met yet)', () => {
    const history = [
      { day: '2023-10-09', playback_secs: goalSecs }
    ];
    const todayPlayback = 0;
    const result = computeStreak(history, goalHrs, todayPlayback, now);
    expect(result).toEqual({ streak: 1, show: true });
  });

  test('streak of 2 if today and yesterday goal met', () => {
    const history = [
      { day: '2023-10-09', playback_secs: goalSecs }
    ];
    const todayPlayback = goalSecs;
    const result = computeStreak(history, goalHrs, todayPlayback, now);
    expect(result).toEqual({ streak: 2, show: true });
  });

  test('streak of 3 with multi-day history', () => {
    const history = [
      { day: '2023-10-09', playback_secs: goalSecs },
      { day: '2023-10-08', playback_secs: goalSecs },
      { day: '2023-10-07', playback_secs: goalSecs - 100 }
    ];
    const todayPlayback = goalSecs;
    const result = computeStreak(history, goalHrs, todayPlayback, now);
    expect(result).toEqual({ streak: 3, show: true });
  });

  test('streak broken if yesterday goal not met and today not met yet', () => {
    const history = [
      { day: '2023-10-09', playback_secs: goalSecs - 1 },
      { day: '2023-10-08', playback_secs: goalSecs }
    ];
    const todayPlayback = 0;
    const result = computeStreak(history, goalHrs, todayPlayback, now);
    expect(result).toEqual({ streak: 0, show: false });
  });

  test('streak continues if yesterday missed but today is met (new streak starts)', () => {
    const history = [
      { day: '2023-10-09', playback_secs: goalSecs - 1 },
      { day: '2023-10-08', playback_secs: goalSecs }
    ];
    const todayPlayback = goalSecs;
    const result = computeStreak(history, goalHrs, todayPlayback, now);
    expect(result).toEqual({ streak: 1, show: true });
  });

  test('handles history with missing days', () => {
    const history = [
      { day: '2023-10-09', playback_secs: goalSecs },
      // 2023-10-08 missing
      { day: '2023-10-07', playback_secs: goalSecs }
    ];
    const todayPlayback = goalSecs;
    const result = computeStreak(history, goalHrs, todayPlayback, now);
    expect(result).toEqual({ streak: 2, show: true });
  });

  test('goal exactly met', () => {
    const result = computeStreak([], goalHrs, goalSecs, now);
    expect(result.streak).toBe(1);
  });

  test('goal not met by 1 second', () => {
    const result = computeStreak([], goalHrs, goalSecs - 1, now);
    expect(result.streak).toBe(0);
  });

  test('todayPlaybackSecs overrides history for today', () => {
    const history = [{ day: '2023-10-10', playback_secs: 100 }];
    const todayPlayback = goalSecs;
    const result = computeStreak(history, goalHrs, todayPlayback, now);
    expect(result.streak).toBe(1);
  });

  test('history for today used if todayPlaybackSecs is null', () => {
    const history = [{ day: '2023-10-10', playback_secs: goalSecs }];
    const result = computeStreak(history, goalHrs, null, now);
    expect(result.streak).toBe(1);
  });
});

import { expect, test, describe } from 'vitest';
import { formatBatteryAgo } from './utils.js';

describe('formatBatteryAgo', () => {
  const now = 1700000000000; // Fixed timestamp for testing

  test('returns 0 sec ago if ms is null or undefined', () => {
    expect(formatBatteryAgo(null)).toBe('0 sec ago');
    expect(formatBatteryAgo(undefined)).toBe('0 sec ago');
    expect(formatBatteryAgo(0)).toBe('0 sec ago');
  });

  test('formats seconds with interval rounding', () => {
    const interval = 10;
    // 5 seconds ago -> rounds down to 0
    expect(formatBatteryAgo(now - 5000, now, interval)).toBe('0 secs ago');
    // 10 seconds ago -> 10
    expect(formatBatteryAgo(now - 10000, now, interval)).toBe('10 secs ago');
    // 15 seconds ago -> 10
    expect(formatBatteryAgo(now - 15000, now, interval)).toBe('10 secs ago');
    // 25 seconds ago -> 20
    expect(formatBatteryAgo(now - 25000, now, interval)).toBe('20 secs ago');
  });

  test('handles singular sec correctly', () => {
    // To get "1 sec ago", stepSeconds must be 1.
    // stepSeconds = Math.floor(seconds / interval) * interval
    // If interval is 1 and seconds is 1, stepSeconds is 1.
    expect(formatBatteryAgo(now - 1000, now, 1)).toBe('1 sec ago');
  });

  test('formats minutes', () => {
    // 60 seconds -> 1 min
    expect(formatBatteryAgo(now - 60000, now)).toBe('1 min ago');
    // 119 seconds -> rounds to 60s -> 1 min
    expect(formatBatteryAgo(now - 119000, now)).toBe('1 min ago');
    // 120 seconds -> 2 mins
    expect(formatBatteryAgo(now - 120000, now)).toBe('2 mins ago');
    // 59 minutes -> 59 mins
    expect(formatBatteryAgo(now - 59 * 60 * 1000, now)).toBe('59 mins ago');
  });

  test('formats hours and minutes', () => {
    // 60 minutes -> 1 hr 0 min
    expect(formatBatteryAgo(now - 60 * 60 * 1000, now)).toBe('1 hr 0 min ago');
    // 90 minutes -> 1 hr 30 min
    expect(formatBatteryAgo(now - 90 * 60 * 1000, now)).toBe('1 hr 30 min ago');
    // 2 hours -> 2 hrs 0 min
    expect(formatBatteryAgo(now - 2 * 60 * 60 * 1000, now)).toBe('2 hrs 0 min ago');
    // 23 hours 59 minutes -> 23 hrs 59 min
    expect(formatBatteryAgo(now - (23 * 60 + 59) * 60 * 1000, now)).toBe('23 hrs 59 min ago');
  });

  test('formats days', () => {
    // 24 hours -> 1 day
    expect(formatBatteryAgo(now - 24 * 60 * 60 * 1000, now)).toBe('1 day ago');
    // 48 hours -> 2 days
    expect(formatBatteryAgo(now - 48 * 60 * 60 * 1000, now)).toBe('2 days ago');
  });

  test('handles negative time as 0 secs ago', () => {
    // If ms is in the future relative to now
    expect(formatBatteryAgo(now + 5000, now)).toBe('0 secs ago');
  });

  test('uses default interval of 10 if not provided', () => {
    // 15 seconds ago with default interval 10 -> 10
    expect(formatBatteryAgo(now - 15000, now)).toBe('10 secs ago');
  });
});

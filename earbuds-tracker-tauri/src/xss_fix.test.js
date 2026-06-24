import { describe, it, expect, vi, beforeAll } from 'vitest';
import fs from 'fs';
import path from 'path';

// We'll read the main.js file and extract the escapeHtml function
// This is more robust than re-defining it.
const mainJsContent = fs.readFileSync(path.resolve(__dirname, 'main.js'), 'utf-8');
const escapeHtmlMatch = mainJsContent.match(/function escapeHtml\(value\) \{([\s\S]*?)\}/);
const escapeHtmlBody = escapeHtmlMatch ? escapeHtmlMatch[1] : '';
const escapeHtml = new Function('value', escapeHtmlBody);

describe('escapeHtml from main.js', () => {
  it('should escape basic HTML tags', () => {
    const input = '<script>alert(1)</script>';
    const expected = '&lt;script&gt;alert(1)&lt;/script&gt;';
    expect(escapeHtml(input)).toBe(expected);
  });

  it('should escape double and single quotes', () => {
    const input = 'brand="XSS"';
    const expected = 'brand=&quot;XSS&quot;';
    expect(escapeHtml(input)).toBe(expected);

    const input2 = "brand='XSS'";
    const expected2 = 'brand=&#39;XSS&#39;';
    expect(escapeHtml(input2)).toBe(expected2);
  });

  it('should escape ampersands', () => {
    const input = 'Sony & Samsung';
    const expected = 'Sony &amp; Samsung';
    expect(escapeHtml(input)).toBe(expected);
  });

  it('should handle null and undefined safely', () => {
    expect(escapeHtml(null)).toBe('');
    expect(escapeHtml(undefined)).toBe('');
  });
});

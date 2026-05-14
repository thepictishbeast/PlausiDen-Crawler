/**
 * supersocietyBadge.ts — shields.io-style SVG badge for the
 * Supersociety Score. T76 cycle 36.
 *
 * Renders a 140×20 px SVG badge that looks like a shields.io
 * `Supersociety: A 97/100` pill. Color-coded by grade.
 * Embeddable in any README via `![Supersociety](...)`.
 *
 * Output is plain SVG, no external CSS or images. The badge
 * is HUMAN-readable (the text isn't a `<text>`-as-image; it's
 * a real `<text>` element with `font-family="system-ui"` so
 * it picks up the OS default), and a11y-readable (the badge
 * exposes a meaningful aria-label).
 *
 * The badge format mirrors shields.io's pattern: a left
 * "label" half and a right "value" half, separated by a
 * vertical line. Left = "supersociety" (grey-ish), right =
 * "A 97/100" (grade colour). Subtle gradient at the top
 * mimics the canonical look. No npm dep on shields.io.
 *
 * Operators put this in their README:
 *
 *     ![Supersociety](path/to/supersociety-badge.svg)
 *
 * And it renders inline. GitHub even renders SVG <text>
 * inside markdown if the file is committed to the repo.
 *
 * REGRESSION-GUARD: all interpolated strings are HTML-escaped
 * via `esc()`. Grade input is constrained to one of A-F via
 * the gradeColor() lookup so an invalid grade falls back to
 * "muted" colouring rather than blowing up.
 */

import type { SupersocietyScore } from './supersocietyScore.js';

function esc(s: unknown): string {
  return String(s)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

const COLORS: Record<string, string> = {
  A: '#22c55e',  // emerald
  B: '#84cc16',  // lime
  C: '#facc15',  // amber (dark text)
  D: '#fb923c',  // orange
  F: '#ef4444',  // red
};

const TEXT_ON_COLOR: Record<string, string> = {
  A: '#0a1a0a',  // dark green text on emerald
  B: '#0a1a0a',
  C: '#1a1500',  // dark on amber
  D: '#1a0a00',
  F: '#1a0000',
};

function gradeColor(grade: string): string {
  return COLORS[grade] ?? '#9990bb';
}

function gradeTextColor(grade: string): string {
  return TEXT_ON_COLOR[grade] ?? '#ffffff';
}

/**
 * Render the badge.
 *
 * Layout (140 × 20 px):
 *   [   supersociety   |   A 97/100   ]
 *           80px            60px
 *
 * Estimated text widths are ~6 px per char at 11px font.
 * "supersociety" = 12 chars × 6 = 72 px + 8 padding = 80.
 * "A 97/100" = up to 8 chars × 6 = 48 + 12 padding = 60.
 *
 * The badge is NOT auto-sized based on text — we keep a
 * fixed 140 px width so it embeds predictably in markdown
 * tables / fixed-width contexts.
 */
export function renderSupersocietyBadge(score: SupersocietyScore): string {
  const W = 140;
  const H = 20;
  const SPLIT = 80;  // x-coordinate of the label/value split.
  const color = gradeColor(score.grade);
  const textColor = gradeTextColor(score.grade);
  const value = `${esc(score.grade)} ${score.composite}/100`;
  const ariaLabel = `Supersociety Score: grade ${score.grade}, ${score.composite} out of 100`;

  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${W} ${H}" width="${W}" height="${H}" role="img" aria-label="${esc(ariaLabel)}">
  <title>${esc(ariaLabel)}</title>
  <linearGradient id="b" x2="0" y2="100%">
    <stop offset="0" stop-color="#bbb" stop-opacity=".1"/>
    <stop offset="1" stop-opacity=".1"/>
  </linearGradient>
  <clipPath id="r">
    <rect width="${W}" height="${H}" rx="3" fill="#fff"/>
  </clipPath>
  <g clip-path="url(#r)">
    <rect width="${SPLIT}" height="${H}" fill="#555"/>
    <rect x="${SPLIT}" width="${W - SPLIT}" height="${H}" fill="${color}"/>
    <rect width="${W}" height="${H}" fill="url(#b)"/>
  </g>
  <g fill="#fff" text-anchor="middle" font-family="system-ui, -apple-system, Segoe UI, Roboto, sans-serif" font-size="11">
    <text x="${SPLIT / 2}" y="15" fill="#010101" fill-opacity=".3">supersociety</text>
    <text x="${SPLIT / 2}" y="14">supersociety</text>
    <text x="${SPLIT + (W - SPLIT) / 2}" y="15" fill="#010101" fill-opacity=".3">${value}</text>
    <text x="${SPLIT + (W - SPLIT) / 2}" y="14" fill="${textColor}">${value}</text>
  </g>
</svg>
`;
}

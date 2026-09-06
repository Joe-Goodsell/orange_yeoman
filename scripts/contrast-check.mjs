// Contrast guard for the editor token palette.
//
// Reads the --oy-* token colours, the plain body colour/background from
// src/app.css (light values in `:root`, dark overrides inside the
// `@media (prefers-color-scheme: dark)` block), and the `.cm-feedback-*`
// background tints from src/lib/Editor.svelte. For each mode it composites
// every tint over the plain background and computes WCAG 2.x contrast ratios
// for every token colour and the body text against every resulting surface.
// Any ratio below 4.5:1 fails the check.
//
// Pure Node ESM, zero dependencies. Run with: node scripts/contrast-check.mjs

import { readFileSync } from "node:fs";

const MIN_RATIO = 4.5;
const TOKEN_NAMES = [
  "--oy-code",
  "--oy-link",
  "--oy-marker",
  "--oy-list",
  "--oy-heading",
];
const TINT_NAMES = [
  "queued",
  "arrived",
  "running",
  "stale",
  "error",
];

const appCssPath = new URL("../src/app.css", import.meta.url);
const editorSveltePath = new URL("../src/lib/Editor.svelte", import.meta.url);

const css = readFileSync(appCssPath, "utf8");
const svelte = readFileSync(editorSveltePath, "utf8");

// --- Parse src/app.css ------------------------------------------------------

function parseDeclarations(block) {
  const out = {};
  // Longest alternatives first so `background-color` is not shadowed by
  // `color`, and `color-scheme` is never mistaken for `color` (it is not
  // followed by a colon).
  const re = /(--oy-[a-z-]+|background-color|color)\s*:\s*([^;]+);/g;
  let m;
  while ((m = re.exec(block))) out[m[1]] = m[2].trim();
  return out;
}

const rootBlock = css.match(/:root\s*\{([^}]*)\}/);
if (!rootBlock) fail("Could not find the :root block in src/app.css");
const lightDecls = parseDeclarations(rootBlock[1]);

const darkMediaBlock = css.match(
  /@media\s*\(prefers-color-scheme:\s*dark\)\s*\{([^}]*)\}/
);
if (!darkMediaBlock)
  fail("Could not find the @media (prefers-color-scheme: dark) block in src/app.css");
const darkDecls = parseDeclarations(darkMediaBlock[1]);

// --- Parse src/lib/Editor.svelte feedback tints ------------------------------

const tints = {};
const ruleRe = /\.cm-feedback-([a-z-]+)[^{]*\{([^}]*)\}/g;
let rule;
let tintRuleCount = 0;
while ((rule = ruleRe.exec(svelte))) {
  const block = rule[2];
  const bgMatch = block.match(/background-color\s*:\s*(rgba?\([^)]+\))\s*;/);
  if (!bgMatch) continue;
  const names = rule[0].match(/cm-feedback-[a-z-]+/g) || [];
  for (const n of names) {
    const key = n.replace(/^cm-feedback-/, "");
    if (TINT_NAMES.includes(key)) {
      tints[key] = parseColor(bgMatch[1]);
      tintRuleCount++;
    }
  }
}

// --- Colour maths (WCAG 2.x) --------------------------------------------------

function parseColor(str) {
  const s = str.trim().toLowerCase();
  let m;
  if ((m = s.match(/^#([0-9a-f]{6})$/))) {
    const n = parseInt(m[1], 16);
    return { r: (n >> 16) & 255, g: (n >> 8) & 255, b: n & 255, a: 1 };
  }
  if ((m = s.match(/^#([0-9a-f]{3})$/))) {
    return {
      r: parseInt(m[1][0] + m[1][0], 16),
      g: parseInt(m[1][1] + m[1][1], 16),
      b: parseInt(m[1][2] + m[1][2], 16),
      a: 1,
    };
  }
  if (
    (m = s.match(
      /^rgba?\(\s*([\d.]+)\s*,\s*([\d.]+)\s*,\s*([\d.]+)(?:\s*,\s*([\d.]+))?\s*\)$/
    ))
  ) {
    return { r: +m[1], g: +m[2], b: +m[3], a: m[4] === undefined ? 1 : +m[4] };
  }
  fail(`Cannot parse colour value: ${str}`);
}

function channelLum(c) {
  c /= 255;
  return c <= 0.03928 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4);
}

function luminance({ r, g, b }) {
  return 0.2126 * channelLum(r) + 0.7152 * channelLum(g) + 0.0722 * channelLum(b);
}

function composite(fg, bg) {
  return {
    r: fg.a * fg.r + (1 - fg.a) * bg.r,
    g: fg.a * fg.g + (1 - fg.a) * bg.g,
    b: fg.a * fg.b + (1 - fg.a) * bg.b,
  };
}

function contrast(a, b) {
  const la = luminance(a);
  const lb = luminance(b);
  const hi = Math.max(la, lb);
  const lo = Math.min(la, lb);
  return (hi + 0.05) / (lo + 0.05);
}

// --- Validate that every expected value was found ------------------------------

for (const [mode, decls] of [
  ["light", lightDecls],
  ["dark", darkDecls],
]) {
  for (const token of TOKEN_NAMES) {
    if (!(token in decls))
      fail(`Missing ${token} in the ${mode} mode declarations of src/app.css`);
  }
  if (!("color" in decls))
    fail(`Missing body color in the ${mode} mode declarations of src/app.css`);
  if (!("background-color" in decls))
    fail(
      `Missing background-color in the ${mode} mode declarations of src/app.css`
    );
}
for (const tint of TINT_NAMES) {
  if (!(tint in tints))
    fail(
      `Missing .cm-feedback-${tint} background tint in src/lib/Editor.svelte`
    );
}

// --- Compute and print the contrast matrix -------------------------------------

const modes = [
  {
    name: "light",
    color: parseColor(lightDecls.color),
    bg: parseColor(lightDecls["background-color"]),
    tokens: Object.fromEntries(
      TOKEN_NAMES.map((t) => [t, parseColor(lightDecls[t])])
    ),
  },
  {
    name: "dark",
    color: parseColor(darkDecls.color),
    bg: parseColor(darkDecls["background-color"]),
    tokens: Object.fromEntries(
      TOKEN_NAMES.map((t) => [t, parseColor(darkDecls[t])])
    ),
  },
];

const fmt = (n) => n.toFixed(2).padStart(6);
const failures = [];

for (const mode of modes) {
  const surfaces = { plain: mode.bg };
  for (const tint of TINT_NAMES) {
    surfaces[`${tint} tint`] = composite(tints[tint], mode.bg);
  }
  const rows = [
    ...TOKEN_NAMES.map((t) => [t, mode.tokens[t]]),
    ["body text", mode.color],
  ];
  const header = `  ${"colour".padEnd(14)}` + Object.keys(surfaces).map((s) => s.padStart(11)).join("");
  console.log(`\n${mode.name.toUpperCase()} mode (bg ${mode.bg.r},${mode.bg.g},${mode.bg.b})`);
  console.log(header);
  for (const [label, colour] of rows) {
    const cells = [];
    for (const [surfaceName, surface] of Object.entries(surfaces)) {
      const ratio = contrast(colour, surface);
      cells.push(fmt(ratio));
      if (ratio < MIN_RATIO) {
        failures.push(
          `${mode.name} mode: ${label} vs ${surfaceName} = ${ratio.toFixed(2)} (below ${MIN_RATIO})`
        );
      }
    }
    console.log(`  ${label.padEnd(14)}` + cells.map((c) => c.padStart(11)).join(""));
  }
}

// --- Report --------------------------------------------------------------------

if (failures.length > 0) {
  console.error("\nContrast check FAILED:");
  for (const f of failures) console.error(`  - ${f}`);
  process.exit(1);
}

// Minimum across the whole matrix for the summary line.
const allRatios = [];
for (const mode of modes) {
  const surfaces = { plain: mode.bg };
  for (const tint of TINT_NAMES) {
    surfaces[`${tint} tint`] = composite(tints[tint], mode.bg);
  }
  for (const colour of [...Object.values(mode.tokens), mode.color]) {
    for (const surface of Object.values(surfaces)) {
      allRatios.push(contrast(colour, surface));
    }
  }
}
const minRatio = Math.min(...allRatios);
console.log(
  `\nContrast OK: ${allRatios.length} ratios checked, minimum ${minRatio.toFixed(2)} (>= ${MIN_RATIO})`
);
process.exit(0);

function fail(message) {
  console.error(`Contrast check error: ${message}`);
  process.exit(1);
}
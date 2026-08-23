#!/usr/bin/env node
// Generates frontend/src/app/tokens.generated.css from design/tokens/halvern.tokens.json.
//
// The JSON is exported from the Figma variable collections and is the single
// source of truth; this script is the bridge that keeps the app's shadcn-named
// variables aliased to it. Edit tokens in Figma, re-export the JSON, re-run
// this. Never tune the generated CSS by hand.
//
// The Figma-token → shadcn-variable mapping below is the table from
// docs/DESIGN_TOKEN_MAPPING.md §2. Three deliberate exceptions do not come
// from the token file and are passed through verbatim; each carries its reason.

import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const tokens = JSON.parse(readFileSync(join(root, "design/tokens/halvern.tokens.json"), "utf8"));

// --- resolve a "stone.950"-style reference into the core table ---
const resolveRef = (ref) => {
  let cur = tokens.core;
  for (const part of ref.split(".")) {
    if (cur == null || typeof cur !== "object" || !(part in cur)) {
      throw new Error(`unresolvable token reference: ${ref}`);
    }
    cur = cur[part];
  }
  if (typeof cur !== "string" || !/^#[0-9a-f]{6}$/i.test(cur)) {
    throw new Error(`reference ${ref} does not resolve to a hex colour: ${cur}`);
  }
  return cur;
};

// --- hex → "H S% L%" triple, matching the hand-conversion's rounding exactly:
// hue to an integer, saturation/lightness to one decimal with trailing .0
// dropped. The equivalence diff against the previous hand-written block
// depends on this formatting, so do not "clean it up".
const hexToHslTriple = (hex) => {
  const r = parseInt(hex.slice(1, 3), 16) / 255;
  const g = parseInt(hex.slice(3, 5), 16) / 255;
  const b = parseInt(hex.slice(5, 7), 16) / 255;
  const max = Math.max(r, g, b), min = Math.min(r, g, b);
  const l = (max + min) / 2;
  let h = 0, s = 0;
  if (max !== min) {
    const d = max - min;
    s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
    if (max === r) h = ((g - b) / d + (g < b ? 6 : 0)) / 6;
    else if (max === g) h = ((b - r) / d + 2) / 6;
    else h = ((r - g) / d + 4) / 6;
  }
  const dec1 = (v) => {
    const x = Math.round(v * 10) / 10;
    return Number.isInteger(x) ? String(x) : x.toFixed(1);
  };
  return `${Math.round(h * 360)} ${dec1(s * 100)}% ${dec1(l * 100)}%`;
};

// --- the §2 mapping: shadcn variable → semantic token ---
const MAPPING = [
  ["background", "bg/app"],
  ["surface", "bg/surface"],
  ["elevated", "bg/elevated"],
  ["foreground", "text/primary"],
  ["card", "bg/overlay"],
  ["card-foreground", "text/primary"],
  ["popover", "bg/overlay"],
  ["popover-foreground", "text/primary"],
  ["primary", "interactive/primary"],
  ["primary-foreground", "interactive/primary-text"],
  ["secondary", "bg/hover"],
  ["secondary-foreground", "text/primary"],
  ["muted", "bg/hover"],
  ["muted-foreground", "text/secondary"],
  ["tertiary", "text/tertiary"],
  // The fourth text level, and the reason it is here rather than expressed as
  // an opacity: opacity multiplies against whatever sits behind, so a control
  // dimmed that way has a different contrast on a card than on the page, and
  // no token can promise anything about it. A named colour is checkable.
  //
  // WCAG exempts disabled controls from the contrast minimum, which is why this
  // one is allowed to sit at 3.25/2.99 — see the $note in the token file. That
  // exemption covers the control's own label. It does not cover explanatory
  // text that happens to be inside a disabled control, which is a design error
  // rather than a contrast decision.
  ["disabled", "text/disabled"],
  ["accent", "accent/subtle"],
  ["accent-foreground", "accent/default"],
  ["destructive", "status/danger/solid"],
  ["border", "border/default"],
  ["input", "border/default"],
  ["ring", "border/focus"],

  // Status triples. Present in the token file from the start but never
  // exported, so panes needing a "this is informational" surface reached for
  // hardcoded blues instead — which is how off-brand colour gets in.
  ["success-bg", "status/success/bg"],
  ["success-border", "status/success/border"],
  ["success-text", "status/success/text"],
  ["warning-bg", "status/warning/bg"],
  ["warning-border", "status/warning/border"],
  ["warning-text", "status/warning/text"],
  ["danger-bg", "status/danger/bg"],
  ["danger-border", "status/danger/border"],
  ["danger-text", "status/danger/text"],
  ["info-bg", "status/info/bg"],
  ["info-border", "status/info/border"],
  ["info-text", "status/info/text"],
];

// --- the exceptions, verbatim, with their reasons ---
const EXCEPTIONS = {
  light: [
    ["destructive-foreground", "9 100% 98.7%", "no on-danger token exists yet; inherited value"],
    ["recording", "357 68% 47.6%", "deliberately vivid — the record indicator must alarm slightly; see DESIGN_TOKEN_MAPPING.md §2"],
    ["chart-1", "12 76% 61%", "no chart tokens exist yet"],
    ["chart-2", "173 58% 39%", "no chart tokens exist yet"],
    ["chart-3", "197 37% 24%", "no chart tokens exist yet"],
    ["chart-4", "43 74% 66%", "no chart tokens exist yet"],
    ["chart-5", "27 87% 67%", "no chart tokens exist yet"],
  ],
  dark: [
    ["destructive-foreground", "9 100% 98.7%", "no on-danger token exists yet; inherited value"],
    ["recording", "359 83% 59.1%", "deliberately vivid — see above"],
    ["chart-1", "220 70% 50%", "no chart tokens exist yet"],
    ["chart-2", "160 60% 45%", "no chart tokens exist yet"],
    ["chart-3", "30 80% 55%", "no chart tokens exist yet"],
    ["chart-4", "280 65% 60%", "no chart tokens exist yet"],
    ["chart-5", "340 75% 55%", "no chart tokens exist yet"],
  ],
};

const emitBlock = (selector, mode) => {
  const lines = [`  ${selector} {`];
  for (const [cssVar, tokenName] of MAPPING) {
    const token = tokens.semantic[tokenName];
    if (!token) throw new Error(`mapping references missing semantic token: ${tokenName}`);
    const hex = resolveRef(token[mode]);
    lines.push(`    --${cssVar}: ${hexToHslTriple(hex)}; /* ${tokenName} */`);
  }
  if (mode === "light") {
    // radius/lg is 8px; the app consumes rem.
    lines.push(`    --radius: ${tokens.core.radius.lg / 16}rem; /* radius/lg */`);
  }
  for (const [cssVar, value, why] of EXCEPTIONS[mode]) {
    lines.push(`    --${cssVar}: ${value}; /* exception: ${why} */`);
  }
  lines.push("  }");
  return lines.join("\n");
};

const css = `/* GENERATED by scripts/build-tokens.mjs from design/tokens/halvern.tokens.json.
   Do not edit — change the Figma variables, re-export the JSON, re-run the script. */

@layer base {
${emitBlock(":root", "light")}

${emitBlock(".dark", "dark")}
}
`;

const out = join(root, "frontend/src/app/tokens.generated.css");
writeFileSync(out, css);
console.log(`wrote ${out} (${css.length} bytes)`);

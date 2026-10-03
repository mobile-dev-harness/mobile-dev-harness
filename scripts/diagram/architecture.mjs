// Generates docs/assets/architecture-{light,dark}.svg: the system architecture in the hand-drawn
// style of Excalidraw, using the same sketching library (Rough.js) and font (Virgil).
//
//   cd scripts/diagram && npm install && npm run build
//
// The Virgil font (© Excalidraw contributors, SIL Open Font License 1.1) is downloaded on first
// run and embedded, so the SVGs render the same everywhere without external requests.

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import rough from "roughjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const out = path.join(here, "../../docs/assets");
const fontUrl = "https://raw.githubusercontent.com/excalidraw/virgil/main/Virgil.woff2";
const fontPath = path.join(here, ".cache/Virgil.woff2");

const W = 1160;
const H = 890;

// Excalidraw's palette.
const themes = {
  light: {
    bg: "#ffffff", ink: "#1e1e1e", muted: "#868e96", note: "#1971c2", hot: "#e03131",
    agent: "#d0bfff", entry: "#ffc9c9", done: "#b2f2bb", planned: "#e9ecef",
    base: "#a5d8ff", device: "#ffec99", helper: "#ffd8a8",
  },
  dark: {
    bg: "#121212", ink: "#e9ecef", muted: "#868e96", note: "#74c0fc", hot: "#ff8787",
    agent: "#5f3dc4", entry: "#a61e4d", done: "#2b8a3e", planned: "#343a40",
    base: "#1864ab", device: "#8f6b00", helper: "#c2410c",
  },
};

let seed = 1;
const gen = rough.generator();

// One decimal is plenty at this size and halves the file.
const round = (d) => d.replace(/-?\d+\.\d+/g, (n) => (+n).toFixed(1).replace(/\.0$/, ""));

function sketch(drawable) {
  return gen.toPaths(drawable).map((p) => {
    const fill = p.fill && p.fill !== "none" ? p.fill : "none";
    return `<path d="${round(p.d)}" stroke="${p.stroke}" stroke-width="${p.strokeWidth}" fill="${fill}" stroke-linecap="round" stroke-linejoin="round"/>`;
  }).join("");
}

function box(t, x, y, w, h, fill, { dashed = false, weight = 1.6 } = {}) {
  return sketch(gen.rectangle(x, y, w, h, {
    seed: seed++, roughness: 1.1, bowing: 1, stroke: t.ink, strokeWidth: weight,
    fill, fillStyle: "hachure", hachureGap: 9, fillWeight: 1.1, hachureAngle: -41,
    strokeLineDash: dashed ? [9, 7] : undefined,
  }));
}

function arrow(t, x1, y1, x2, y2, { color = t.ink, dashed = false, both = false } = {}) {
  const opts = { seed: seed++, roughness: 0.9, stroke: color, strokeWidth: 1.8, strokeLineDash: dashed ? [8, 6] : undefined };
  const head = (fx, fy, tx, ty) => {
    const a = Math.atan2(ty - fy, tx - fx);
    const l = 14;
    return sketch(gen.line(tx, ty, tx - l * Math.cos(a - 0.45), ty - l * Math.sin(a - 0.45), { ...opts, strokeLineDash: undefined })) +
      sketch(gen.line(tx, ty, tx - l * Math.cos(a + 0.45), ty - l * Math.sin(a + 0.45), { ...opts, strokeLineDash: undefined }));
  };
  return sketch(gen.line(x1, y1, x2, y2, opts)) + head(x1, y1, x2, y2) + (both ? head(x2, y2, x1, y1) : "");
}

function text(t, x, y, lines, { size = 20, color = t.ink, anchor = "middle", rotate = 0 } = {}) {
  const ls = Array.isArray(lines) ? lines : [lines];
  const lh = size * 1.25;
  const y0 = y - ((ls.length - 1) * lh) / 2;
  const tr = rotate ? ` transform="rotate(${rotate} ${x} ${y})"` : "";
  const spans = ls.map((l, i) => `<tspan x="${x}" y="${(y0 + i * lh).toFixed(1)}">${esc(l)}</tspan>`).join("");
  return `<text font-size="${size}" fill="${color}" text-anchor="${anchor}" dominant-baseline="middle"${tr}>${spans}</text>`;
}

const esc = (s) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

/** A labelled box: title plus smaller lines. */
function card(t, x, y, w, h, fill, title, lines = [], opts = {}) {
  const titleY = lines.length ? y + 30 : y + h / 2;
  let s = box(t, x, y, w, h, fill, opts) + text(t, x + w / 2, titleY, title, { size: opts.titleSize ?? 22 });
  if (lines.length) {
    s += text(t, x + w / 2, y + 30 + 18 + (lines.length * 19) / 2, lines, { size: 16, color: opts.dashed ? t.muted : t.ink });
  }
  return s;
}

function diagram(t) {
  seed = 1;
  const parts = [];

  // 1. Who uses it
  parts.push(card(t, 60, 30, 400, 86, t.agent, "Coding agents", ["Claude Code · Codex · Cursor · …"]));
  parts.push(card(t, 700, 30, 400, 86, t.agent, "People · CI · scripts", ["shell, --json"]));

  // 2. Entry points
  parts.push(arrow(t, 260, 120, 260, 170));
  parts.push(text(t, 274, 146, "MCP over stdio", { size: 16, anchor: "start", color: t.muted }));
  parts.push(arrow(t, 900, 120, 900, 170));
  parts.push(text(t, 914, 146, "CLI", { size: 16, anchor: "start", color: t.muted }));
  parts.push(card(t, 110, 174, 300, 60, t.entry, "mdh mcp"));
  parts.push(card(t, 750, 174, 300, 60, t.entry, "mdh"));
  parts.push(text(t, 580, 204, "same engine, same compact text", { size: 17, color: t.note }));
  parts.push(arrow(t, 260, 238, 260, 286));
  parts.push(arrow(t, 900, 238, 900, 286));

  // 3. Control, then verification (ADR-0009): the engine with its check kinds, inside the
  // compatibility matrix that repeats it across devices.
  parts.push(card(t, 40, 300, 250, 170, t.done, "Control ✓", ["session · stable refs", "act → wait → diff", "logs · crashes · run"]));
  parts.push(arrow(t, 334, 385, 296, 385));
  parts.push(box(t, 340, 290, 790, 190, "none", { dashed: true, weight: 1.4 }));
  parts.push(text(t, 360, 310, "compatibility matrix — the same flows and checks on every device & config", { size: 16, anchor: "start", color: t.muted }));
  parts.push(box(t, 360, 326, 750, 140, t.planned, { weight: 1.4 }));
  parts.push(text(t, 378, 346, "verification engine — flows · verdicts · evidence · baselines ✓", { size: 17, anchor: "start" }));
  const checks = [["Functional", ["assertions on", "screens & logs"]],
    ["UI consistency", ["baselines · configs", "layout & a11y rules"]],
    ["Performance", ["startup · jank", "memory · CPU"]]];
  checks.forEach(([title, lines], i) => {
    parts.push(card(t, 378 + i * 242, 362, 228, 92, t.done, `${title} ✓`, lines, { titleSize: 19 }));
  });
  parts.push(text(t, 1110, 310, "dashed = planned", { size: 15, anchor: "end", color: t.muted }));

  // 4. Shared foundation
  parts.push(arrow(t, 580, 484, 580, 520));
  parts.push(box(t, 30, 524, 1100, 130, "none", { weight: 1.2 }));
  parts.push(text(t, 50, 546, "shared foundation", { size: 17, anchor: "start", color: t.muted }));
  parts.push(card(t, 50, 562, 250, 78, t.base, "mdh-observe", ["tree · refs · diffs · logs"], { titleSize: 20 }));
  parts.push(card(t, 315, 562, 200, 78, t.base, "mdh-project", ["Gradle · diagnostics"], { titleSize: 20 }));
  parts.push(card(t, 530, 562, 225, 78, t.base, "mdh-impact", ["change → screens"], { titleSize: 20 }));
  parts.push(card(t, 770, 562, 190, 78, t.base, "mdh-driver", ["adb · helper"], { titleSize: 20 }));
  parts.push(card(t, 975, 562, 135, 78, t.base, "mdh-core", ["types · errors"], { titleSize: 20 }));

  // 5. The device
  parts.push(box(t, 30, 700, 1100, 170, t.device, { weight: 1.6 }));
  parts.push(text(t, 50, 848, "Android emulator or device", { size: 17, anchor: "start" }));
  parts.push(card(t, 90, 742, 200, 80, t.bg, "logcat", [], { titleSize: 22 }));
  parts.push(card(t, 470, 742, 220, 80, t.bg, "your app", [], { titleSize: 22 }));
  parts.push(card(t, 790, 742, 310, 80, t.helper, "dev.mdh.helper", ["UiAutomation, always warm"], { titleSize: 20 }));

  // Data paths
  parts.push(arrow(t, 865, 644, 925, 738, { both: true }));
  parts.push(text(t, 905, 684, "JSON via adb forward · ~10 ms", { size: 16, anchor: "start", color: t.note }));
  parts.push(arrow(t, 694, 782, 786, 782, { both: true }));
  parts.push(text(t, 740, 760, "UI · input", { size: 14, color: t.muted }));
  parts.push(arrow(t, 466, 782, 294, 782, { color: t.muted, dashed: true }));
  parts.push(text(t, 380, 760, "logs", { size: 14, color: t.muted }));
  parts.push(arrow(t, 190, 738, 190, 644, { dashed: true }));
  parts.push(text(t, 206, 690, "crashes · ANRs · errors", { size: 16, anchor: "start", color: t.hot }));

  // Margin notes
  parts.push(text(t, 580, 74, ["~150 tokens", "per screen"], { size: 18, color: t.note, rotate: -4 }));
  parts.push(text(t, 580, 254, "every action returns only what changed", { size: 16, color: t.note }));

  return `<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="${H}" viewBox="0 0 ${W} ${H}">
<!-- Generated by scripts/diagram/architecture.mjs. Font: Virgil, © Excalidraw contributors, SIL Open Font License 1.1. -->
<style>@font-face{font-family:Virgil;src:url(data:font/woff2;base64,${font}) format("woff2");}text{font-family:Virgil,"Comic Sans MS",cursive;}</style>
<rect width="100%" height="100%" fill="${t.bg}"/>
${parts.join("\n")}
</svg>
`;
}

if (!fs.existsSync(fontPath)) {
  fs.mkdirSync(path.dirname(fontPath), { recursive: true });
  const res = await fetch(fontUrl);
  if (!res.ok) throw new Error(`could not download Virgil: ${res.status}`);
  fs.writeFileSync(fontPath, Buffer.from(await res.arrayBuffer()));
}
const font = fs.readFileSync(fontPath).toString("base64");

fs.mkdirSync(out, { recursive: true });
for (const [name, theme] of Object.entries(themes)) {
  const file = path.join(out, `architecture-${name}.svg`);
  fs.writeFileSync(file, diagram(theme));
  console.log(`wrote ${path.relative(process.cwd(), file)} (${Math.round(fs.statSync(file).size / 1024)} KB)`);
}

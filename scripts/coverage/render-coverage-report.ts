import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";

interface CoverageMetric {
  readonly covered: number;
  readonly found: number;
}

interface CoverageFile {
  readonly functions: CoverageMetric;
  readonly lines: CoverageMetric;
  readonly path: string;
}

/** Parses Bun LCOV output into file coverage records. */
export function parseLcov(lcov: string): CoverageFile[] {
  const files: CoverageFile[] = [];
  let current: { functions: CoverageMetric; lines: CoverageMetric; path: string } | undefined;

  for (const line of lcov.split(/\r?\n/u)) {
    if (line.startsWith("SF:")) {
      current = {
        functions: { covered: 0, found: 0 },
        lines: { covered: 0, found: 0 },
        path: line.slice(3),
      };
      continue;
    }
    if (!current) continue;
    if (line.startsWith("FNF:")) current.functions = { ...current.functions, found: value(line) };
    if (line.startsWith("FNH:")) current.functions = { ...current.functions, covered: value(line) };
    if (line.startsWith("LF:")) current.lines = { ...current.lines, found: value(line) };
    if (line.startsWith("LH:")) current.lines = { ...current.lines, covered: value(line) };
    if (line === "end_of_record") {
      files.push(current);
      current = undefined;
    }
  }
  return files;
}

/** Renders the coverage index and the first labeled TypeScript page. */
export function renderCoverageReport(
  lcovPath = join("coverage", "lcov.info"),
  outputRoot = "coverage",
  updatedAt = new Date().toISOString(),
): void {
  const files = parseLcov(readFileSync(lcovPath, "utf8"));
  const publicationDate = coverageUpdatedAt(updatedAt);
  const reportPath = join(outputRoot, "typescript", "index.html");
  mkdirSync(dirname(reportPath), { recursive: true });
  writeFileSync(reportPath, reportHtml(files, publicationDate));
  writeFileSync(join(outputRoot, "index.html"), indexHtml(publicationDate));
}

function value(line: string): number {
  return Number(line.split(":")[1] ?? 0);
}

function percent(metric: CoverageMetric): string {
  return metric.found === 0 ? "100.00" : ((metric.covered / metric.found) * 100).toFixed(2);
}

function total(files: CoverageFile[], key: "functions" | "lines"): CoverageMetric {
  return files.reduce(
    (result, file) => ({
      covered: result.covered + file[key].covered,
      found: result.found + file[key].found,
    }),
    { covered: 0, found: 0 },
  );
}

function html(value: string): string {
  return value.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;");
}

/**
 * Normalizes a project-owned coverage publication time to ISO UTC.
 *
 * @param value - Candidate timestamp.
 * @returns Canonical ISO UTC timestamp.
 */
export function coverageUpdatedAt(value: string): string {
  const timestamp = new Date(value);

  if (Number.isNaN(timestamp.getTime())) {
    throw new Error(`Invalid coverage publication date: ${value}`);
  }

  return timestamp.toISOString();
}

/**
 * Formats the publication timestamp shown on every coverage page.
 *
 * @param value - Canonical ISO UTC timestamp.
 * @returns Human-readable UTC timestamp.
 */
function coverageUpdatedAtLabel(value: string): string {
  const label = new Intl.DateTimeFormat("en-US", {
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
    month: "short",
    timeZone: "UTC",
    year: "numeric",
  }).format(new Date(value));

  return `${label} UTC`;
}

const coverageThemeSchemes = {
  atlas: {
    accent: "#0f6b7a",
    accentSoft: "#e4f3f5",
    bg: "#f4f6f8",
    border: "#d8dee8",
    colorScheme: "light",
    muted: "#667085",
    panel: "#ffffff",
    text: "#17202a",
  },
  paper: {
    accent: "#68737a",
    accentSoft: "#ecefed",
    bg: "#f6f6f3",
    border: "#dcdfdc",
    colorScheme: "light",
    muted: "#697176",
    panel: "#ffffff",
    text: "#1f2528",
  },
  citrine: {
    accent: "#766f18",
    accentSoft: "#eeebc7",
    bg: "#f7f6ea",
    border: "#dedbb8",
    colorScheme: "light",
    muted: "#70705c",
    panel: "#fffef8",
    text: "#20231a",
  },
  harbor: {
    accent: "#35b8cd",
    accentSoft: "#0e2f3a",
    bg: "#111a24",
    border: "#2a3a4c",
    colorScheme: "dark",
    muted: "#7d92a8",
    panel: "#1a2636",
    text: "#dde4ee",
  },
  midnight: {
    accent: "#5fc0ee",
    accentSoft: "#0d3040",
    bg: "#06111a",
    border: "#1f3547",
    colorScheme: "dark",
    muted: "#89a6b8",
    panel: "#0b1a24",
    text: "#eaf6ff",
  },
  onyx: {
    accent: "#8fb4ff",
    accentSoft: "#182234",
    bg: "#0b0d10",
    border: "#2a3139",
    colorScheme: "dark",
    muted: "#9aa4ad",
    panel: "#14181d",
    text: "#edf0f2",
  },
  rose: {
    accent: "#9e4c58",
    accentSoft: "#f1e6e8",
    bg: "#fbf6f7",
    border: "#e2d2d5",
    colorScheme: "light",
    muted: "#74676b",
    panel: "#ffffff",
    text: "#241b1e",
  },
  tide: {
    accent: "#3f82a8",
    accentSoft: "#e4f0f6",
    bg: "#f2f8fb",
    border: "#d2e2ea",
    colorScheme: "light",
    muted: "#627584",
    panel: "#ffffff",
    text: "#17242c",
  },
  ember: {
    accent: "#df6532",
    accentSoft: "#ffe8d8",
    bg: "#fff7e8",
    border: "#efd8bd",
    colorScheme: "light",
    muted: "#7a6658",
    panel: "#fffdf9",
    text: "#251a12",
  },
  quartz: {
    accent: "#7c6f9f",
    accentSoft: "#eeeaf8",
    bg: "#f7f5fb",
    border: "#ddd7ed",
    colorScheme: "light",
    muted: "#706b7a",
    panel: "#ffffff",
    text: "#211f29",
  },
} as const;

function themeCss(): string {
  const themes = Object.entries(coverageThemeSchemes)
    .map(
      ([scheme, tokens]) =>
        `:root[data-scheme="${scheme}"] { color-scheme: ${tokens.colorScheme}; --bg: ${tokens.bg}; --panel: ${tokens.panel}; --text: ${tokens.text}; --muted: ${tokens.muted}; --border: ${tokens.border}; --accent: ${tokens.accent}; --accent-soft: ${tokens.accentSoft}; }`,
    )
    .join("\n");
  const atlas = coverageThemeSchemes.atlas;
  return `:root { color-scheme: ${atlas.colorScheme}; --bg: ${atlas.bg}; --panel: ${atlas.panel}; --text: ${atlas.text}; --muted: ${atlas.muted}; --border: ${atlas.border}; --accent: ${atlas.accent}; --accent-soft: ${atlas.accentSoft}; }\n${themes}`;
}

function themeScript(): string {
  return `<script>(() => { const schemes = new Set(["atlas", "paper", "citrine", "harbor", "midnight", "onyx", "rose", "tide", "ember", "quartz"]); const keys = ["connorhunter.theme.scheme", "portfolio.theme.scheme"]; const fallback = window.matchMedia?.("(prefers-color-scheme: dark)").matches ? "midnight" : "atlas"; let scheme; for (const key of keys) { try { const value = localStorage.getItem(key); if (schemes.has(value)) { scheme = value; break; } } catch {} } document.documentElement.dataset.scheme = scheme || fallback; window.addEventListener("message", (event) => { const message = event.data; if (!message || typeof message !== "object" || !schemes.has(message.scheme) || (typeof message.type === "string" && !message.type.endsWith(".theme.scheme"))) return; document.documentElement.dataset.scheme = message.scheme; }); })();</script>`;
}

function page(title: string, content: string, indexHref: string, updatedAt: string): string {
  const typeScriptHref = indexHref === "index.html" ? "typescript/index.html" : "index.html";
  return `<!doctype html>
<html data-scheme="atlas" lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>${title}</title>
<style>${themeCss()}*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--text);font:.9375rem/1.5 ui-sans-serif,system-ui,-apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif}main{width:min(100%,72rem);margin:0 auto;padding:clamp(1.25rem,4vw,3rem)}header{margin-bottom:1.25rem}h1{margin:0;font-size:clamp(1.75rem,4vw,2.75rem);line-height:1.05}p{margin:.5rem 0 0;color:var(--muted)}a{color:var(--accent);font-weight:700;text-underline-offset:.2em}nav{display:flex;gap:.5rem;flex-wrap:wrap;margin-bottom:.5rem}nav a{border:1px solid var(--border);border-radius:999px;background:var(--panel);padding:.45rem .75rem;text-decoration:none}nav a[aria-current="page"]{border-color:var(--accent);background:var(--accent-soft)}.coverage-updated{margin:0 0 1.25rem;color:var(--muted);font-size:.8rem;font-weight:700}.table-wrap{overflow:auto;border:1px solid var(--border);border-radius:.5rem;background:var(--panel)}table{width:100%;min-width:36rem;border-collapse:collapse}th,td{border-bottom:1px solid var(--border);padding:.85rem 1rem;text-align:left;vertical-align:top}thead th{color:var(--muted);font-size:.75rem;letter-spacing:.04em;text-transform:uppercase}tbody tr:first-child{background:color-mix(in srgb,var(--accent) 10%,transparent);font-weight:800}tbody tr:last-child th,tbody tr:last-child td{border-bottom:0}@media(max-width:600px){main{padding:1.25rem .75rem}table{min-width:31rem}th,td{padding:.7rem .75rem}}</style>${themeScript()}</head>
<body><main><nav aria-label="Coverage pages"><a ${indexHref === "index.html" ? 'aria-current="page"' : ""} href="${indexHref}">Overview</a><a ${indexHref === "index.html" ? "" : 'aria-current="page"'} href="${typeScriptHref}">TypeScript</a></nav><p class="coverage-updated">Updated <time datetime="${html(updatedAt)}">${html(coverageUpdatedAtLabel(updatedAt))}</time></p>${content}</main></body></html>`;
}

function indexHtml(updatedAt: string): string {
  return page(
    "Cipher coverage",
    '<header><h1>Cipher coverage</h1><p>Available reports are grouped by code surface.</p></header><div class="table-wrap"><table><thead><tr><th>Page</th><th>Scope</th></tr></thead><tbody><tr><td><a href="typescript/index.html">TypeScript</a></td><td>Desktop UI, tooling, and infrastructure scripts</td></tr><tr><td>Rust</td><td>Coverage tooling is not selected yet.</td></tr></tbody></table></div>',
    "index.html",
    updatedAt,
  );
}

function reportHtml(files: CoverageFile[], updatedAt: string): string {
  const rows = files
    .sort((left, right) => left.path.localeCompare(right.path))
    .map(
      (file) =>
        `<tr><th>${html(file.path)}</th><td>${percent(file.lines)}% (${file.lines.covered}/${file.lines.found})</td><td>${percent(file.functions)}% (${file.functions.covered}/${file.functions.found})</td></tr>`,
    )
    .join("");
  const lines = total(files, "lines");
  const functions = total(files, "functions");
  return page(
    "Cipher TypeScript coverage",
    `<header><h1>TypeScript coverage</h1><p>Generated from the Bun test suite.</p></header><div class="table-wrap"><table><thead><tr><th>File</th><th>Lines</th><th>Functions</th></tr></thead><tbody><tr><th>All files</th><td>${percent(lines)}% (${lines.covered}/${lines.found})</td><td>${percent(functions)}% (${functions.covered}/${functions.found})</td></tr>${rows}</tbody></table></div>`,
    "../index.html",
    updatedAt,
  );
}

if (import.meta.main) renderCoverageReport();

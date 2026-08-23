import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";

import { focusRouteContent } from "../src/app/focus-restoration";
import { desktopThemeSchemeClassifications, desktopThemeSchemes } from "../src/desktop-contract";
import { cn } from "../src/lib/utils";

const styles = readFileSync("src/styles.css", "utf8");
const shell = readFileSync("src/app/desktop-shell.tsx", "utf8");
const router = readFileSync("src/routes/index.tsx", "utf8");
const themeProvider = readFileSync("src/features/theme/theme-provider.tsx", "utf8");
const viteConfig = readFileSync("vite.config.ts", "utf8");

describe("desktop design system", () => {
  test("uses the Tailwind v4 Vite integration and source-owned UI primitives", () => {
    expect(viteConfig).toContain('from "@tailwindcss/vite"');
    expect(viteConfig).toContain("tailwindcss()");
    expect(styles).toContain('@import "tailwindcss"');
    for (const primitive of [
      "button.tsx",
      "badge.tsx",
      "card.tsx",
      "input.tsx",
      "label.tsx",
      "separator.tsx",
      "select.tsx",
      "visually-hidden.tsx",
    ]) {
      expect(readFileSync(join("src/components/ui", primitive), "utf8")).toContain(
        "export function",
      );
    }
    expect(readFileSync("src/components/ui/button.tsx", "utf8")).toContain(
      "class-variance-authority",
    );
    expect(readFileSync("src/lib/utils.ts", "utf8")).toContain("tailwind-merge");
  });

  test("defines complete semantic tokens for every light and dark scheme", () => {
    const tokens = [
      "canvas",
      "surface",
      "elevated",
      "border",
      "text",
      "muted",
      "focus",
      "accent",
      "destructive",
      "on-destructive",
      "success",
      "warning",
    ] as const;
    for (const scheme of desktopThemeSchemes) {
      const block = schemeTokenBlock(scheme);
      for (const token of tokens) {
        expect(block).toContain(`--cipher-${token}:`);
      }
      expect(block).toContain(`color-scheme: ${desktopThemeSchemeClassifications[scheme]}`);
    }
    expect(styles).toContain("forced-colors: active");
    for (const role of [
      "display",
      "page-title",
      "section-title",
      "body",
      "body-muted",
      "label",
      "caption",
      "code",
      "numeric",
    ]) {
      expect(styles).toContain(`.type-${role}`);
    }
    expect(styles).toContain("--cipher-paragraph-spacing:");
  });

  test("keeps essential text, action, and focus pairs above contrast thresholds", () => {
    for (const scheme of desktopThemeSchemes) {
      const block = schemeTokenBlock(scheme);
      const canvas = tokenColor(block, "canvas");
      const surface = tokenColor(block, "surface");
      expect(contrast(tokenColor(block, "text"), canvas)).toBeGreaterThanOrEqual(7);
      expect(contrast(tokenColor(block, "muted"), canvas)).toBeGreaterThanOrEqual(4.5);
      expect(
        contrast(tokenColor(block, "on-accent"), tokenColor(block, "accent")),
      ).toBeGreaterThanOrEqual(4.5);
      expect(
        contrast(tokenColor(block, "on-destructive"), tokenColor(block, "destructive")),
      ).toBeGreaterThanOrEqual(4.5);
      expect(contrast(tokenColor(block, "focus"), surface)).toBeGreaterThanOrEqual(3);
    }
  });

  test("keeps native resolution, focus, motion, and narrow layout behavior explicit", () => {
    expect(themeProvider).toContain("desktopThemeBoundary");
    expect(themeProvider).not.toContain("matchMedia");
    expect(themeProvider).not.toMatch(/(?:localStorage|sessionStorage)\.(?:getItem|setItem)/u);
    expect(styles).toContain("prefers-reduced-motion: reduce");
    expect(styles).toContain("grid-template-areas");
    expect(styles).toContain(".secondary-rail");
    expect(styles).toContain("display: none");
    expect(router).toContain("createMemoryHistory");
    expect(router).toContain('declare module "@tanstack/react-router"');
  });

  test("retains semantic focus and keyboard affordances in the shell", () => {
    expect(shell).toContain('href="#main-content"');
    expect(shell).toContain('aria-label="Primary"');
    expect(shell).toContain("data-tauri-drag-region");
    expect(shell).toContain("ThemePreferenceControl");

    const calls: FocusOptions[] = [];
    focusRouteContent({
      focus: (options?: FocusOptions): void => void calls.push(options ?? {}),
    } as HTMLElement);
    expect(calls).toEqual([{ preventScroll: true }]);
    expect(cn("border border-transparent", "border-border")).toBe("border border-border");
    expect(cn("type-label text-text", "text-muted")).toBe("type-label text-muted");
  });
});

function schemeTokenBlock(scheme: string): string {
  const match = styles.match(new RegExp(`:root\\[data-scheme="${scheme}"\\] \\{([^}]*)\\}`, "u"));
  if (match?.[1] === undefined) {
    throw new Error(`Missing scheme block: ${scheme}`);
  }
  return match[1];
}

function tokenColor(block: string, token: string): string {
  const match = block.match(new RegExp(`--cipher-${token}:\\s*(#[0-9a-f]{6})`, "iu"));
  if (match?.[1] === undefined) {
    throw new Error(`Missing color token: ${token}`);
  }
  return match[1];
}

function contrast(first: string, second: string): number {
  const [lighter, darker] = [luminance(first), luminance(second)].sort(
    (left, right) => right - left,
  );
  return (lighter + 0.05) / (darker + 0.05);
}

function luminance(color: string): number {
  const channels = [1, 3, 5].map(
    (offset) => Number.parseInt(color.slice(offset, offset + 2), 16) / 255,
  );
  const [red = 0, green = 0, blue = 0] = channels.map((channel) =>
    channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4,
  );
  return 0.2126 * red + 0.7152 * green + 0.0722 * blue;
}

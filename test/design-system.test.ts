import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";

import { focusRouteContent } from "../src/app/focus-restoration";
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

  test("defines complete semantic light and dark token sets", () => {
    for (const token of [
      "canvas",
      "surface",
      "elevated",
      "border",
      "text",
      "muted",
      "focus",
      "accent",
      "destructive",
      "success",
      "warning",
    ]) {
      expect(styles.match(new RegExp(`--cipher-${token}:`, "gu"))?.length).toBeGreaterThanOrEqual(
        2,
      );
    }
    expect(styles).toContain(':root[data-theme="light"]');
    expect(styles).toContain(':root[data-theme="dark"]');
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

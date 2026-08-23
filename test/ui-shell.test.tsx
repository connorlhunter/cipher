import { afterEach, describe, expect, test } from "bun:test";
import { Window } from "happy-dom";
import { type JSX } from "react";

import { Card } from "../src/components/ui/card";
import { Input } from "../src/components/ui/input";
import { Label } from "../src/components/ui/label";
import {
  ThemePreferenceControl,
  nextThemePreference,
} from "../src/features/theme/theme-preference-control";
import {
  ThemeProvider,
  applyDesktopTheme,
  type NativeThemeBoundary,
} from "../src/features/theme/theme-provider";

const browser = new Window({ url: "http://cipher.localhost" });
Object.assign(globalThis, {
  Event: browser.Event,
  HTMLElement: browser.HTMLElement,
  MouseEvent: browser.MouseEvent,
  Node: browser.Node,
  document: browser.document,
  navigator: browser.navigator,
  scrollTo: () => undefined,
  window: browser,
});

const { cleanup, render, screen } = await import("@testing-library/react");
const userEvent = (await import("@testing-library/user-event")).default;
const { RouterProvider } = await import("@tanstack/react-router");
const { router } = await import("../src/routes");
const { RouteFocusFrame } = await import("../src/app/focus-restoration");

function shell(boundary: NativeThemeBoundary): JSX.Element {
  return (
    <ThemeProvider boundary={boundary}>
      <ThemePreferenceControl />
    </ThemeProvider>
  );
}

afterEach(() => {
  cleanup();
  browser.document.documentElement.removeAttribute("data-theme");
  browser.document.documentElement.removeAttribute("data-scheme");
  browser.document.documentElement.removeAttribute("data-theme-preference");
  browser.document.documentElement.style.colorScheme = "";
});

describe("theme preference UI", () => {
  test("selects and cycles native schemes without browser persistence", async () => {
    const selected: string[] = [];
    const boundary: NativeThemeBoundary = {
      current: async () => ({ preference: "system", scheme: "midnight", resolved: "dark" }),
      set: async (preference) => {
        selected.push(preference);
        return preference === "harbor" || preference === "midnight" || preference === "onyx"
          ? { preference, scheme: preference, resolved: "dark" }
          : {
              preference,
              scheme: preference === "system" ? "atlas" : preference,
              resolved: "light",
            };
      },
      subscribe: async () => () => undefined,
    };

    render(shell(boundary));
    const user = userEvent.setup({ document: browser.document as unknown as Document });
    await user.selectOptions(await screen.findByRole("combobox", { name: "Color scheme" }), "rose");

    expect(selected).toEqual(["rose"]);
    expect(browser.document.documentElement.dataset.theme).toBe("light");
    expect(browser.document.documentElement.dataset.scheme).toBe("rose");
    expect(browser.document.documentElement.dataset.themePreference).toBe("rose");
    expect(browser.document.querySelector("output")?.textContent).toContain(
      "Rose preference active with the Rose light scheme",
    );

    await user.click(screen.getByRole("button", { name: "Use Tide appearance" }));
    expect(selected).toEqual(["rose", "tide"]);
    expect(browser.document.documentElement.dataset.scheme).toBe("tide");
  });

  test("cycles through system and every explicit scheme in a stable order", () => {
    const visited: string[] = [];
    let current: Parameters<typeof nextThemePreference>[0] = "system";
    for (let index = 0; index < 11; index += 1) {
      current = nextThemePreference(current);
      visited.push(current);
    }
    expect(visited).toEqual([
      "atlas",
      "paper",
      "citrine",
      "harbor",
      "midnight",
      "onyx",
      "rose",
      "tide",
      "ember",
      "quartz",
      "system",
    ]);
  });

  test("writes resolved theme attributes only to the current document", () => {
    applyDesktopTheme(browser.document.documentElement as unknown as HTMLElement, {
      preference: "onyx",
      scheme: "onyx",
      resolved: "dark",
    });

    expect(browser.document.documentElement.dataset).toMatchObject({
      scheme: "onyx",
      theme: "dark",
      themePreference: "onyx",
    });
    expect(browser.document.documentElement.style.colorScheme).toBe("dark");
  });

  test("keeps the last safe appearance and announces an unavailable native update", async () => {
    const boundary: NativeThemeBoundary = {
      current: async () => ({ preference: "system", scheme: "midnight", resolved: "dark" }),
      set: async () => {
        throw new Error("native theme unavailable");
      },
      subscribe: async () => () => undefined,
    };

    render(shell(boundary));
    const user = userEvent.setup({ document: browser.document as unknown as Document });
    await user.selectOptions(
      await screen.findByRole("combobox", { name: "Color scheme" }),
      "atlas",
    );

    expect(browser.document.documentElement.dataset.theme).toBe("dark");
    expect(browser.document.documentElement.dataset.scheme).toBe("midnight");
    expect(browser.document.querySelector("output")?.textContent).toContain(
      "Appearance controls are unavailable",
    );
  });

  test("restores main-content focus after keyboard-safe route navigation", () => {
    const { rerender } = render(
      <RouteFocusFrame pathname="/overview">
        <h1>Overview</h1>
      </RouteFocusFrame>,
    );

    rerender(
      <RouteFocusFrame pathname="/settings/appearance">
        <h1>Appearance</h1>
      </RouteFocusFrame>,
    );

    expect(browser.document.activeElement?.id).toBe("main-content");
    expect(screen.getByRole("heading", { name: "Appearance" })).toBeDefined();
  });

  test("renders reviewed form primitives with accessible label association", () => {
    render(
      <Card aria-label="Form surface">
        <Label htmlFor="field">Field</Label>
        <Input id="field" name="field" />
      </Card>,
    );

    expect(screen.getByRole("region", { name: "Form surface" })).toBeDefined();
    expect(screen.getByLabelText("Field").getAttribute("id")).toBe("field");
  });
});

describe("desktop shell accessibility", () => {
  test("keeps landmarks, skip navigation, route focus, and controls keyboard reachable", async () => {
    const boundary: NativeThemeBoundary = {
      current: async () => ({ preference: "system", scheme: "atlas", resolved: "light" }),
      set: async (preference) =>
        preference === "harbor" || preference === "midnight" || preference === "onyx"
          ? { preference, scheme: preference, resolved: "dark" }
          : {
              preference,
              scheme: preference === "system" ? "atlas" : preference,
              resolved: "light",
            },
      subscribe: async () => () => undefined,
    };
    await router.navigate({ to: "/" });
    render(
      <ThemeProvider boundary={boundary}>
        <RouterProvider router={router} />
      </ThemeProvider>,
    );

    expect(await screen.findByRole("banner")).toBeDefined();
    expect(screen.getByRole("navigation", { name: "Primary" })).toBeDefined();
    expect(screen.getByRole("main")).toBeDefined();
    expect(screen.getByRole("complementary", { name: "Workspace context" })).toBeDefined();

    const user = userEvent.setup({ document: browser.document as unknown as Document });
    await user.tab();
    expect(browser.document.activeElement?.textContent).toBe("Skip to content");
    await user.keyboard("{Enter}");
    expect(browser.document.activeElement?.id).toBe("main-content");
    await user.click(screen.getByRole("link", { name: "Appearance" }));

    expect(await screen.findByRole("heading", { level: 1, name: "Appearance" })).toBeDefined();
    expect(browser.document.activeElement?.id).toBe("main-content");
    expect(screen.getAllByRole("group", { name: "Appearance" })).toHaveLength(2);
  });
});

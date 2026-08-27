import { afterEach, describe, expect, test } from "bun:test";
import { Window } from "happy-dom";
import { type JSX } from "react";

import { Card } from "../src/components/ui/card";
import { Input } from "../src/components/ui/input";
import { Label } from "../src/components/ui/label";
import type { DesktopAuthenticationRequest } from "../src/desktop";
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

const { act, cleanup, render, screen } = await import("@testing-library/react");
const userEvent = (await import("@testing-library/user-event")).default;
const { QueryClient, QueryClientProvider } = await import("@tanstack/react-query");
const { RouterProvider, createMemoryHistory, createRootRoute, createRoute, createRouter } =
  await import("@tanstack/react-router");
const { router } = await import("../src/routes");
const { RouteFocusFrame } = await import("../src/app/focus-restoration");
const { PasswordResetForm, SignInForm, SignUpForm } =
  await import("../src/features/auth/sign-in-form");
const { AppearanceRoute, ChangelogRoute, DeviceRoute, OverviewRoute, UninstallRoute } =
  await import("../src/routes");

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
    expect(screen.getByRole("img", { name: "End-to-end encryption" }).getAttribute("title")).toBe(
      "End-to-end encrypted: only you and the people you message can read your conversations. Your account uses an encryption key generated and securely stored on your device to encrypt your data. The key never leaves your device and is deleted when you uninstall the app. Your data is also deleted when you delete your account. Not even Cipher staff can access or read your encrypted conversations.",
    );

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

  test("centers sign-in around a debounced identifier check before enabling the password field", async () => {
    render(
      <QueryClientProvider client={new QueryClient()}>
        <SignInForm />
      </QueryClientProvider>,
    );

    const user = userEvent.setup({ document: browser.document as unknown as Document });
    const identifier = screen.getByLabelText("Email or username");
    const password = screen.getByLabelText("Password");
    expect((password as HTMLInputElement).disabled).toBe(true);

    await user.type(identifier, "not valid");
    await act(async () => {
      await Bun.sleep(400);
    });
    expect(screen.getByText("Enter a valid email or username.")).toBeDefined();
    expect((password as HTMLInputElement).disabled).toBe(true);

    await user.clear(identifier);
    await user.type(identifier, "cipher.user");
    expect(screen.getByText("Checking…")).toBeDefined();
    await act(async () => {
      await Bun.sleep(400);
    });
    expect(screen.getByText("Looks good.")).toBeDefined();
    expect((password as HTMLInputElement).disabled).toBe(false);
    expect((screen.getByRole("button", { name: "Sign in" }) as HTMLButtonElement).disabled).toBe(
      true,
    );
    await user.type(password, "Strong-password1!");
    expect((screen.getByRole("button", { name: "Sign in" }) as HTMLButtonElement).disabled).toBe(
      false,
    );
  });

  test("keeps credentials transient while showing loading, failure, and verification states", async () => {
    const requests: Array<Record<string, string>> = [];
    let resolveSignIn: ((value: { state: "failed"; message: string }) => void) | undefined;
    render(
      <QueryClientProvider client={new QueryClient()}>
        <SignInForm
          authenticate={async (request) => {
            requests.push(request);
            if (request.flow === "sign_in") {
              return new Promise((resolve) => {
                resolveSignIn = resolve;
              });
            }
            return { state: "authenticated", message: "Signed in securely." };
          }}
        />
      </QueryClientProvider>,
    );
    const user = userEvent.setup({ document: browser.document as unknown as Document });
    const identifier = screen.getByLabelText("Email or username");
    await user.type(identifier, "cipher.user");
    await act(async () => {
      await Bun.sleep(400);
    });
    await user.type(screen.getByLabelText("Password"), "Strong-password1!");
    await user.click(screen.getByRole("button", { name: "Sign in" }));
    expect(screen.getByRole("button", { name: "Signing in…" })).toBeDefined();
    await act(async () => resolveSignIn?.({ state: "failed", message: "Try again." }));
    expect(screen.getByRole("alert").textContent).toContain("Try again.");
    expect((identifier as HTMLInputElement).value).toBe("");
    expect(requests[0]).toEqual({
      flow: "sign_in",
      identifier: "cipher.user",
      password: "Strong-password1!",
    });
  });

  test("moves from sign-in to a one-time verification form and clears each submission", async () => {
    const requests: Array<Record<string, string>> = [];
    render(
      <QueryClientProvider client={new QueryClient()}>
        <SignInForm
          authenticate={async (request) => {
            requests.push(request);
            return request.flow === "sign_in"
              ? { state: "challenge_required", message: "Enter the verification code to continue." }
              : { state: "authenticated", message: "Signed in securely." };
          }}
        />
      </QueryClientProvider>,
    );
    const user = userEvent.setup({ document: browser.document as unknown as Document });
    await user.type(screen.getByLabelText("Email or username"), "cipher.user");
    await act(async () => {
      await Bun.sleep(400);
    });
    await user.type(screen.getByLabelText("Password"), "Strong-password1!");
    await user.click(screen.getByRole("button", { name: "Sign in" }));
    const code = await screen.findByLabelText("Verification code");
    await user.type(code, "123456");
    await user.click(screen.getByRole("button", { name: "Verify" }));
    await act(async () => {
      await Promise.resolve();
    });
    expect(screen.getByRole("status").textContent).toContain("Signed in securely.");
    expect(screen.queryByLabelText("Verification code")).toBeNull();
    expect(requests.map((request) => request.flow)).toEqual(["sign_in", "continue_challenge"]);
  });

  test("keeps password recovery as its own credential-clearing form", async () => {
    const requests: Array<Record<string, string>> = [];
    render(
      <QueryClientProvider client={new QueryClient()}>
        <PasswordResetForm
          authenticate={async (request) => {
            requests.push(request);
            return {
              state: "password_reset_required",
              message: "If an account is eligible, a recovery code is on its way.",
            };
          }}
        />
      </QueryClientProvider>,
    );

    const user = userEvent.setup({ document: browser.document as unknown as Document });
    const identifier = screen.getByLabelText("Email or username");
    await user.type(identifier, "cipher.user");
    await user.click(screen.getByRole("button", { name: "Send recovery code" }));

    expect(requests).toEqual([{ flow: "begin_password_reset", identifier: "cipher.user" }]);
    expect(await screen.findByLabelText("Recovery code")).toBeDefined();
    expect((screen.getByLabelText("Email or username") as HTMLInputElement).value).toBe("");
  });

  test("keeps overview and appearance copy short and centered on the project", async () => {
    const rootRoute = createRootRoute();
    const overviewRoute = createRoute({
      getParentRoute: () => rootRoute,
      path: "/",
      component: OverviewRoute,
    });
    const overviewRouter = createRouter({
      history: createMemoryHistory({ initialEntries: ["/"] }),
      routeTree: rootRoute.addChildren([overviewRoute]),
    });
    render(<RouterProvider router={overviewRouter} />);
    expect(await screen.findByRole("img", { name: "Cipher" })).toBeDefined();
    expect(
      screen.getByText("A private space for the conversations that matter most."),
    ).toBeDefined();
    expect(screen.getByText("Built for E2EE")).toBeDefined();
    expect(screen.getByRole("link", { name: "Log in" }).getAttribute("href")).toBe("/sign-in");
    expect(screen.getByRole("link", { name: "Sign up" }).getAttribute("href")).toBe("/sign-up");
    cleanup();
    render(
      <ThemeProvider
        boundary={{
          current: async () => ({ preference: "system", scheme: "atlas", resolved: "light" }),
          set: async () => ({ preference: "system", scheme: "atlas", resolved: "light" }),
          subscribe: async () => () => undefined,
        }}
      >
        <AppearanceRoute />
      </ThemeProvider>,
    );
    expect(screen.getByText("Choose the look that feels right for you.")).toBeDefined();
    expect(screen.queryByText("Voice")).toBeNull();
    cleanup();
    render(<DeviceRoute />);
    expect(screen.getByText("Voice")).toBeDefined();
    expect(screen.getByText("Video")).toBeDefined();
    expect(screen.getByText("Notifications")).toBeDefined();
    cleanup();
    render(<ChangelogRoute />);
    expect(screen.getAllByText("Changelog").length).toBeGreaterThan(0);
    cleanup();
    render(<UninstallRoute />);
    expect(screen.getByRole("button", { name: "Uninstall Cipher" })).toBeDefined();
  });

  test("keeps the uninstall fallback contained to an installed Cipher app", async () => {
    render(<UninstallRoute />);
    const user = userEvent.setup({ document: browser.document as unknown as Document });

    await user.click(screen.getByRole("button", { name: "Uninstall Cipher" }));
    expect(screen.getByRole("group", { name: "Confirm uninstall" })).toBeDefined();
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.getByRole("button", { name: "Uninstall Cipher" })).toBeDefined();

    await user.click(screen.getByRole("button", { name: "Uninstall Cipher" }));
    const removeCredentials = screen.getByRole("checkbox", {
      name: "Remove saved credentials from this device",
    }) as HTMLInputElement;
    expect(removeCredentials.checked).toBe(true);
    await user.click(removeCredentials);
    expect(removeCredentials.checked).toBe(false);
    await user.click(screen.getByRole("button", { name: "Uninstall now" }));

    expect((await screen.findByRole("status")).textContent).toContain(
      "Uninstall is available from an installed Cipher app.",
    );
  });

  test("uses the dedicated invitation route without retaining account credentials", async () => {
    const requests: DesktopAuthenticationRequest[] = [];
    render(
      <QueryClientProvider client={new QueryClient()}>
        <SignUpForm
          authenticate={async (request) => {
            requests.push(request);
            return { state: "authenticated", message: "Your account is ready." };
          }}
        />
      </QueryClientProvider>,
    );

    const user = userEvent.setup({ document: browser.document as unknown as Document });
    await user.type(screen.getByLabelText("Email or username"), "cipher.user");
    await user.type(screen.getByLabelText("Temporary password"), "TemporaryPassphrase");
    await user.type(screen.getByLabelText("New password"), "A much stronger password");
    await user.click(screen.getByRole("button", { name: "Create account" }));

    expect(requests).toEqual([
      {
        flow: "accept_administrator_invitation",
        identifier: "cipher.user",
        temporaryPassword: "TemporaryPassphrase",
        newPassword: "A much stronger password",
      },
    ]);
    expect((screen.getByLabelText("Temporary password") as HTMLInputElement).value).toBe("");
    expect((screen.getByLabelText("New password") as HTMLInputElement).value).toBe("");
  });
});

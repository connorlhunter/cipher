import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

import {
  desktopDiagnosticsWith,
  desktopStatusWith,
  desktopThemeWith,
  listenForDesktopThemeChangesWith,
  parseDesktopDiagnostics,
  parseDesktopStatus,
  parseDesktopTheme,
  setDesktopThemeWith,
} from "../src/desktop";
import {
  desktopCommands,
  desktopThemeChangedEvent,
  desktopProtocol,
  maxDesktopStatusMessageLength,
  supportsDesktopProtocol,
} from "../src/desktop-contract";

describe("parseDesktopStatus", () => {
  test("accepts a native desktop status", () => {
    expect(parseDesktopStatus({ message: "Desktop core is ready." })).toEqual({
      message: "Desktop core is ready.",
    });
  });

  test.each([null, {}, { message: "" }, { message: 1 }])("rejects %p", (value) => {
    expect(() => parseDesktopStatus(value)).toThrow("invalid status");
  });

  test("validates the status returned by the native command", async () => {
    await expect(
      desktopStatusWith(async (command, arguments_) => ({
        message:
          command === desktopCommands.status &&
          arguments_.protocolVersion === desktopProtocol.current
            ? "ready"
            : "",
      })),
    ).resolves.toEqual({ message: "ready" });
  });

  test("rejects secrets and unbounded status payloads", () => {
    expect(() => parseDesktopStatus({ message: "ready", token: "forbidden" })).toThrow(
      "invalid status",
    );
    expect(() =>
      parseDesktopStatus({ message: "x".repeat(maxDesktopStatusMessageLength + 1) }),
    ).toThrow("invalid status");
  });

  test("accepts the current and previous desktop protocol versions", () => {
    expect(supportsDesktopProtocol(desktopProtocol.previous)).toBe(true);
    expect(supportsDesktopProtocol(desktopProtocol.current)).toBe(true);
    expect(supportsDesktopProtocol(desktopProtocol.current + 1)).toBe(false);
  });

  test("keeps current and previous fixture responses compatible", () => {
    for (const fixturePath of [
      "contracts/ipc/v0/desktop-status.json",
      "contracts/ipc/v1/desktop-status.json",
    ]) {
      const fixture = JSON.parse(readFileSync(fixturePath, "utf8")) as {
        protocolVersion: number;
        command: string;
        response: unknown;
      };

      expect(supportsDesktopProtocol(fixture.protocolVersion)).toBe(true);
      expect(fixture.command).toBe(desktopCommands.status);
      expect(parseDesktopStatus(fixture.response)).toEqual({ message: "Desktop core is ready." });
    }
  });
});

describe("parseDesktopDiagnostics", () => {
  const diagnostic = {
    lifecycleState: "active",
    transportState: "ready",
    rendererEpoch: 1,
    activeOperations: 0,
    coldStarts: 1,
    wakes: 0,
  } as const;

  test("accepts the bounded diagnostics returned by the native command", async () => {
    expect(parseDesktopDiagnostics(diagnostic)).toEqual(diagnostic);
    await expect(
      desktopDiagnosticsWith(async (command, arguments_) =>
        command === desktopCommands.diagnostics &&
        arguments_.protocolVersion === desktopProtocol.current
          ? diagnostic
          : {},
      ),
    ).resolves.toEqual(diagnostic);
  });

  test.each([
    null,
    {},
    { ...diagnostic, token: "forbidden" },
    { ...diagnostic, lifecycleState: "unknown" },
    { ...diagnostic, activeOperations: 33 },
    { ...diagnostic, rendererEpoch: -1 },
    { ...diagnostic, wakes: Number.MAX_SAFE_INTEGER + 1 },
  ])("rejects non-display diagnostics %p", (value) => {
    expect(() => parseDesktopDiagnostics(value)).toThrow("invalid diagnostics");
  });

  test("keeps the version-one diagnostic fixture compatible", () => {
    const fixture = JSON.parse(
      readFileSync("contracts/ipc/v1/desktop-diagnostics.json", "utf8"),
    ) as { command: string; protocolVersion: number; response: unknown };

    expect(fixture.command).toBe(desktopCommands.diagnostics);
    expect(fixture.protocolVersion).toBe(desktopProtocol.current);
    expect(parseDesktopDiagnostics(fixture.response)).toEqual(diagnostic);
  });
});

describe("desktop theme boundary", () => {
  const theme = { preference: "system", resolved: "dark" } as const;

  test("accepts only the native-owned preference and resolved appearance", async () => {
    expect(parseDesktopTheme(theme)).toEqual(theme);
    await expect(
      desktopThemeWith(async (command, arguments_) =>
        command === desktopCommands.theme && arguments_.protocolVersion === desktopProtocol.current
          ? theme
          : {},
      ),
    ).resolves.toEqual(theme);

    const fixture = JSON.parse(readFileSync("contracts/ipc/v1/desktop-theme.json", "utf8")) as {
      command: string;
      protocolVersion: number;
      response: unknown;
    };
    expect(fixture.command).toBe(desktopCommands.theme);
    expect(fixture.protocolVersion).toBe(desktopProtocol.current);
    expect(parseDesktopTheme(fixture.response)).toEqual(theme);
  });

  test.each([
    null,
    {},
    { preference: "browser", resolved: "dark" },
    { preference: "system", resolved: "auto" },
    { preference: "system", resolved: "dark", token: "forbidden" },
  ])("rejects an unsafe native theme view: %p", (value) => {
    expect(() => parseDesktopTheme(value)).toThrow("invalid theme");
  });

  test("sends one bounded preference to the native command", async () => {
    await expect(
      setDesktopThemeWith(async (command, arguments_) => {
        expect(command).toBe(desktopCommands.setTheme);
        expect(arguments_).toEqual({
          preference: "light",
          protocolVersion: desktopProtocol.current,
        });
        return { preference: "light", resolved: "light" };
      }, "light"),
    ).resolves.toEqual({ preference: "light", resolved: "light" });
  });

  test("keeps native theme notifications content-free and injectable", async () => {
    let eventName = "";
    let handler: (() => void) | undefined;
    let refreshes = 0;
    const stop = await listenForDesktopThemeChangesWith(
      async (nextEventName, nextHandler) => {
        eventName = nextEventName;
        handler = nextHandler;
        return () => undefined;
      },
      async () => {
        refreshes += 1;
      },
    );

    expect(eventName).toBe(desktopThemeChangedEvent);
    handler?.();
    await Promise.resolve();
    expect(refreshes).toBe(1);
    await stop();
  });
});

import { describe, expect, test } from "bun:test";

import { desktopStatusWith, parseDesktopStatus } from "../src/desktop";

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
      desktopStatusWith(async (command) => ({
        message: command === "desktop_status" ? "ready" : "",
      })),
    ).resolves.toEqual({ message: "ready" });
  });
});

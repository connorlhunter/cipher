import { describe, expect, test } from "bun:test";

import { parseDesktopStatus } from "../src/desktop";

describe("parseDesktopStatus", () => {
  test("accepts a native desktop status", () => {
    expect(parseDesktopStatus({ message: "Desktop core is ready." })).toEqual({
      message: "Desktop core is ready.",
    });
  });

  test.each([null, {}, { message: "" }, { message: 1 }])("rejects %p", (value) => {
    expect(() => parseDesktopStatus(value)).toThrow("invalid status");
  });
});

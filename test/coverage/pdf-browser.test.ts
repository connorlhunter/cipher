import { expect, test } from "bun:test";

import { pdfBrowserLaunchOptions } from "../../scripts/coverage/pdf-browser";

test("selects safe browser options for local and CI PDF rendering", () => {
  expect(pdfBrowserLaunchOptions(false)).toEqual({ args: [], headless: true });
  expect(pdfBrowserLaunchOptions(true)).toEqual({
    args: ["--no-sandbox", "--disable-setuid-sandbox"],
    headless: true,
  });
});

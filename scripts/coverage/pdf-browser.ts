import type { LaunchOptions } from "puppeteer";

/**
 * Returns browser options for local and hosted PDF rendering.
 *
 * @param continuousIntegration - Whether Chrome runs in an isolated CI job.
 * @returns Puppeteer launch options.
 */
export function pdfBrowserLaunchOptions(continuousIntegration: boolean): LaunchOptions {
  return {
    args: continuousIntegration ? ["--no-sandbox", "--disable-setuid-sandbox"] : [],
    headless: true,
  };
}

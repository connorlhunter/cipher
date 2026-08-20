import { existsSync, mkdirSync } from "node:fs";
import { dirname } from "node:path";
import { pathToFileURL } from "node:url";
import puppeteer from "puppeteer";
import { coveragePaths } from "./coverage-paths";
import { pdfBrowserLaunchOptions } from "./pdf-browser";

/** PDF paths produced for one coverage publication. */
export interface RenderedCoveragePdfs {
  readonly overview: string;
  readonly typescript: string;
}

/** Narrow PDF options needed by the coverage renderer. */
export interface CoveragePdfOptions {
  readonly format?: "Letter";
  readonly landscape?: boolean;
  readonly margin?: {
    readonly bottom?: string;
    readonly left?: string;
    readonly right?: string;
    readonly top?: string;
  };
  readonly path?: string;
  readonly printBackground?: boolean;
}

/** Page operations used while printing a coverage report. */
export interface CoveragePdfPage {
  close(): Promise<void>;
  emulateMediaType(type?: string): Promise<void>;
  goto(url: string, options?: { readonly waitUntil?: "networkidle0" }): Promise<unknown>;
  pdf(options?: CoveragePdfOptions): Promise<Uint8Array>;
}

/** Browser operations used while printing coverage reports. */
export interface CoveragePdfBrowser {
  close(): Promise<void>;
  newPage(): Promise<CoveragePdfPage>;
}

/** Opens the browser used to print coverage reports. */
export type CoveragePdfBrowserLauncher = () => Promise<CoveragePdfBrowser>;

/** Optional collaborators for PDF rendering. */
export interface RenderCoveragePdfsOptions {
  readonly launchBrowser?: CoveragePdfBrowserLauncher;
}

/**
 * Renders Cipher's overview and TypeScript coverage pages as PDFs.
 *
 * @param workspaceRoot - Cipher checkout containing the coverage pages.
 * @returns Both generated PDF paths.
 */
export async function renderCoveragePdfs(
  workspaceRoot = process.cwd(),
  options: RenderCoveragePdfsOptions = {},
): Promise<RenderedCoveragePdfs> {
  const paths = coveragePaths(workspaceRoot);

  for (const input of [paths.overview.html, paths.typescript.html]) {
    if (!existsSync(input)) {
      throw new Error(`Missing coverage report: ${input}. Render coverage HTML first.`);
    }
  }

  const launchBrowser =
    options.launchBrowser ??
    (() => puppeteer.launch(pdfBrowserLaunchOptions(process.env.CI === "true")));
  const browser = await launchBrowser();

  try {
    await renderPdf(browser, paths.overview.html, paths.overview.pdf);
    await renderPdf(browser, paths.typescript.html, paths.typescript.pdf);
  } finally {
    await browser.close();
  }

  console.log(`Rendered coverage PDFs: ${paths.overview.pdf}, ${paths.typescript.pdf}`);

  return { overview: paths.overview.pdf, typescript: paths.typescript.pdf };
}

/**
 * Prints one local HTML page with the shared browser instance.
 *
 * @param browser - Open Puppeteer browser.
 * @param input - HTML report path.
 * @param output - PDF destination.
 */
async function renderPdf(
  browser: CoveragePdfBrowser,
  input: string,
  output: string,
): Promise<void> {
  mkdirSync(dirname(output), { recursive: true });
  const page = await browser.newPage();

  try {
    await page.emulateMediaType("print");
    await page.goto(pathToFileURL(input).href, { waitUntil: "networkidle0" });
    await page.pdf({
      format: "Letter",
      landscape: true,
      margin: {
        bottom: "0.45in",
        left: "0.45in",
        right: "0.45in",
        top: "0.45in",
      },
      path: output,
      printBackground: true,
    });
  } finally {
    await page.close();
  }
}

if (import.meta.main) {
  try {
    await renderCoveragePdfs();
  } catch (error) {
    console.error(error instanceof Error ? error.message : error);
    process.exit(1);
  }
}

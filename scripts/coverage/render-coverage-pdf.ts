import { createWriteStream, existsSync, mkdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import PDFDocument from "pdfkit";
import type { CoverageArtifact, CoverageMetric } from "./render-coverage-report";

export interface RenderedCoveragePdfs {
  readonly overview: string;
}

function metricText(metric: CoverageMetric): string {
  const percentage = metric.found === 0 ? 100 : (metric.covered / metric.found) * 100;
  return `${percentage.toFixed(2)}% (${metric.covered}/${metric.found})`;
}

/** Renders Cipher coverage from its JSON artifact without a browser. */
export async function renderCoveragePdfs(
  workspaceRoot = process.cwd(),
): Promise<RenderedCoveragePdfs> {
  const directory = join(workspaceRoot, "coverage");
  const json = join(directory, "index.json");
  const pdf = join(directory, "coverage.pdf");
  if (!existsSync(json)) throw new Error(`Missing coverage artifact: ${json}.`);
  const coverage = JSON.parse(readFileSync(json, "utf8")) as CoverageArtifact;
  mkdirSync(dirname(pdf), { recursive: true });

  await new Promise<void>((resolve, reject) => {
    const document = new PDFDocument({
      info: { Title: "Cipher Coverage" },
      margin: 48,
      size: "LETTER",
    });
    const stream = createWriteStream(pdf);
    document.pipe(stream);
    stream.on("finish", resolve);
    stream.on("error", reject);
    document.font("Helvetica-Bold").fontSize(22).fillColor("#17202a").text("Cipher Coverage");
    document
      .moveDown(0.35)
      .font("Helvetica")
      .fontSize(10)
      .fillColor("#667085")
      .text(
        `Updated ${coverage.updatedAt}. Required minimum: ${coverage.minimumCoverage.lines}% lines and ${coverage.minimumCoverage.functions}% functions.`,
      );
    for (const surface of coverage.surfaces) {
      document
        .moveDown(0.9)
        .font("Helvetica-Bold")
        .fontSize(14)
        .fillColor("#0f6b7a")
        .text(surface.label);
      document
        .moveDown(0.25)
        .font("Helvetica")
        .fontSize(10)
        .fillColor("#17202a")
        .text(
          `All files: lines ${metricText(surface.totals.lines)}, functions ${metricText(surface.totals.functions)}`,
        );
      for (const file of surface.files) {
        document
          .moveDown(0.2)
          .fontSize(9)
          .text(
            `${file.path}: lines ${metricText(file.lines)}, functions ${metricText(file.functions)}`,
          );
      }
    }
    document.end();
  });
  console.log(`Rendered coverage PDF: ${pdf}`);
  return { overview: pdf };
}

if (import.meta.main) {
  try {
    await renderCoveragePdfs();
  } catch (error) {
    console.error(error instanceof Error ? error.message : error);
    process.exit(1);
  }
}

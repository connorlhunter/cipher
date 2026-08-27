import { createWriteStream, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import PDFDocument from "pdfkit";

export interface ChangelogSection {
  readonly entries: ReadonlyArray<string>;
  readonly title: string;
}

export interface ChangelogRelease {
  readonly date: string;
  readonly sections: ReadonlyArray<ChangelogSection>;
  readonly version: string;
}

export interface ChangelogPaths {
  readonly directory: string;
  readonly markdown: string;
  readonly pdf: string;
}

const releaseHeading = /^##\s+\[?([^\]\s]+)\]?\s+-\s+(\d{4}-\d{2}-\d{2})\s*$/u;
const sectionHeading = /^###\s+(.+?)\s*$/u;
const entryLine = /^\s*[-*]\s+(.+?)\s*$/u;

/** Resolves the local changelog publication directory. */
export function changelogPaths(workspaceRoot = process.cwd()): ChangelogPaths {
  const directory = join(workspaceRoot, "changelog");
  return {
    directory,
    markdown: join(directory, "CHANGELOG.md"),
    pdf: join(directory, "changelog.pdf"),
  };
}

/** Parses canonical CHANGELOG.md without creating a second release source. */
export function parseChangelog(markdown: string): ChangelogRelease[] {
  const releases: Array<{ date: string; sections: ChangelogSection[]; version: string }> = [];
  let release: { date: string; sections: ChangelogSection[]; version: string } | undefined;
  let section: { entries: string[]; title: string } | undefined;
  for (const line of markdown.split(/\r?\n/u)) {
    const releaseMatch = releaseHeading.exec(line);
    if (releaseMatch) {
      release = { date: releaseMatch[2] ?? "", sections: [], version: releaseMatch[1] ?? "" };
      releases.push(release);
      section = undefined;
      continue;
    }
    if (!release) continue;
    const sectionMatch = sectionHeading.exec(line);
    if (sectionMatch) {
      section = { entries: [], title: sectionMatch[1] ?? "Changes" };
      release.sections.push(section);
      continue;
    }
    const entryMatch = entryLine.exec(line);
    if (entryMatch) {
      const target = section ?? { entries: [], title: "Changes" };
      if (!section) {
        release.sections.push(target);
        section = target;
      }
      target.entries.push(entryMatch[1] ?? "");
    }
  }
  if (releases.length === 0) throw new Error("CHANGELOG.md does not contain a release heading.");
  return releases;
}

/** Requires package.json and the newest changelog release to match. */
export function assertCurrentRelease(
  version: string,
  releases: ReadonlyArray<ChangelogRelease>,
): void {
  if (releases[0]?.version !== version) throw new Error(`CHANGELOG.md must begin with ${version}.`);
}

/** Builds Markdown and PDF changelog artifacts from this repository's source changelog. */
export async function buildChangelogArtifact(
  workspaceRoot = process.cwd(),
  publishedAt = new Date().toISOString(),
): Promise<ChangelogPaths> {
  const paths = changelogPaths(workspaceRoot);
  const packageJson = JSON.parse(readFileSync(join(workspaceRoot, "package.json"), "utf8")) as {
    version: string;
  };
  const markdown = readFileSync(join(workspaceRoot, "CHANGELOG.md"), "utf8");
  const releases = parseChangelog(markdown);
  assertCurrentRelease(packageJson.version, releases);
  const normalizedPublishedAt = new Date(publishedAt).toISOString();
  rmSync(paths.directory, { force: true, recursive: true });
  mkdirSync(paths.directory, { recursive: true });
  writeFileSync(paths.markdown, markdown);
  await new Promise<void>((resolve, reject) => {
    const document = new PDFDocument({
      info: { Title: "Cipher Changelog" },
      margin: 48,
      size: "LETTER",
    });
    const stream = createWriteStream(paths.pdf);
    document.pipe(stream);
    stream.on("finish", resolve);
    stream.on("error", reject);
    document.font("Helvetica-Bold").fontSize(22).fillColor("#17202a").text("Cipher Changelog");
    document
      .moveDown(0.35)
      .font("Helvetica")
      .fontSize(10)
      .fillColor("#667085")
      .text(`Published ${normalizedPublishedAt}`);
    for (const release of releases) {
      document
        .moveDown(0.9)
        .font("Helvetica-Bold")
        .fontSize(14)
        .fillColor("#0f6b7a")
        .text(`v${release.version}`);
      document
        .moveDown(0.2)
        .font("Helvetica")
        .fontSize(10)
        .fillColor("#667085")
        .text(`Released ${release.date}`);
      for (const section of release.sections) {
        document
          .moveDown(0.55)
          .font("Helvetica-Bold")
          .fontSize(11)
          .fillColor("#17202a")
          .text(section.title);
        for (const entry of section.entries)
          document.moveDown(0.2).font("Helvetica").fontSize(10).text(`• ${entry}`);
      }
    }
    document.end();
  });
  return paths;
}

if (import.meta.main) {
  try {
    await buildChangelogArtifact();
  } catch (error) {
    console.error(error instanceof Error ? error.message : error);
    process.exit(1);
  }
}

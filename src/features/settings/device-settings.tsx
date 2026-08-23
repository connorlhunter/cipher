import { Bell, Mic, Newspaper, Trash2, Video } from "lucide-react";
import { marked, type Token, type Tokens } from "marked";
import { type JSX, type ReactNode, useState } from "react";

// TypeScript's native preview does not yet resolve Vite raw-asset query modules.
// @ts-ignore The Vite build resolves this repository-owned source file.
import changelog from "../../../CHANGELOG.md?raw";
import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import { Card } from "../../components/ui/card";
import { desktopRemoveCipher } from "../../desktop";

interface PlaceholderSettingProps {
  icon: typeof Mic;
  title: string;
}

function PlaceholderSetting({ icon: Icon, title }: PlaceholderSettingProps): JSX.Element {
  return (
    <div className="flex min-h-control items-center gap-3 rounded-md px-1">
      <Icon aria-hidden="true" className="shrink-0 text-muted" size={18} strokeWidth={1.8} />
      <span className="type-label text-text">{title}</span>
      <Badge className="ml-auto" tone="neutral">
        Coming soon
      </Badge>
    </div>
  );
}

function isSafeChangelogLink(href: string): boolean {
  return /^(?:https?:|mailto:)/iu.test(href);
}

function tokenText(token: Token): string {
  return "text" in token && typeof token.text === "string" ? token.text : token.raw;
}

function renderInline(tokens: Token[], keyPrefix: string): ReactNode[] {
  return tokens.map((token, index) => {
    const key = `${keyPrefix}-${index}`;
    const children =
      "tokens" in token && token.tokens ? renderInline(token.tokens, key) : tokenText(token);

    switch (token.type) {
      case "strong":
        return <strong key={key}>{children}</strong>;
      case "em":
        return <em key={key}>{children}</em>;
      case "del":
        return <del key={key}>{children}</del>;
      case "codespan":
        return (
          <code className="rounded bg-elevated px-1 font-mono" key={key}>
            {(token as Tokens.Codespan).text}
          </code>
        );
      case "br":
        return <br key={key} />;
      case "link":
        return isSafeChangelogLink((token as Tokens.Link).href) ? (
          <a
            className="text-accent underline underline-offset-2"
            href={(token as Tokens.Link).href}
            key={key}
          >
            {children}
          </a>
        ) : (
          <span key={key}>{children}</span>
        );
      case "text":
      case "escape":
        return <span key={key}>{children}</span>;
      default:
        return <span key={key}>{token.raw}</span>;
    }
  });
}

function renderChangelog(): ReactNode[] {
  return marked.lexer(changelog).map((token, index) => {
    const key = `changelog-${index}`;
    switch (token.type) {
      case "heading": {
        const heading = token as Tokens.Heading;
        const content = renderInline(heading.tokens, key);
        if (heading.depth === 1) {
          return (
            <h2 className="type-section-title" key={key}>
              {content}
            </h2>
          );
        }
        if (heading.depth === 2) {
          return (
            <h3 className="type-label mt-section text-text" key={key}>
              {content}
            </h3>
          );
        }
        return (
          <h4 className="type-label mt-paragraph text-text" key={key}>
            {content}
          </h4>
        );
      }
      case "paragraph":
        return (
          <p className="type-caption mt-paragraph" key={key}>
            {renderInline((token as Tokens.Paragraph).tokens, key)}
          </p>
        );
      case "list": {
        const list = token as Tokens.List;
        const List = list.ordered ? "ol" : "ul";
        return (
          <List className="type-caption mt-2 list-outside pl-5 marker:text-muted" key={key}>
            {list.items.map((item: Tokens.ListItem, itemIndex: number) => (
              <li className="mt-1" key={`${key}-${itemIndex}`}>
                {item.tokens.map((itemToken: Token, tokenIndex: number) =>
                  itemToken.type === "paragraph"
                    ? renderInline(
                        (itemToken as Tokens.Paragraph).tokens,
                        `${key}-${itemIndex}-${tokenIndex}`,
                      )
                    : renderInline([itemToken], `${key}-${itemIndex}-${tokenIndex}`),
                )}
              </li>
            ))}
          </List>
        );
      }
      case "space":
        return null;
      default:
        return (
          <p className="type-caption mt-paragraph" key={key}>
            {token.raw}
          </p>
        );
    }
  });
}

function UninstallCipher(): JSX.Element {
  const [confirming, setConfirming] = useState(false);
  const [removeLocalData, setRemoveLocalData] = useState(true);
  const [isPending, setIsPending] = useState(false);
  const [unavailable, setUnavailable] = useState(false);

  const startRemoval = async (): Promise<void> => {
    setUnavailable(false);
    setIsPending(true);
    try {
      await desktopRemoveCipher(removeLocalData);
    } catch {
      setUnavailable(true);
      setIsPending(false);
    }
  };

  if (!confirming) {
    return (
      <Button onClick={() => setConfirming(true)} variant="destructive">
        <Trash2 aria-hidden="true" size={17} strokeWidth={1.8} />
        Uninstall Cipher
      </Button>
    );
  }

  return (
    <div className="grid gap-3" role="group" aria-label="Confirm uninstall">
      <label className="type-label flex items-start gap-3 text-text">
        <input
          checked={removeLocalData}
          className="mt-1 size-4 accent-accent"
          onChange={(event) => setRemoveLocalData(event.currentTarget.checked)}
          type="checkbox"
        />
        <span>Remove saved credentials from this device</span>
      </label>
      <p className="type-caption text-muted">
        This removes saved refresh credentials from Keychain or Credential Manager, but does not
        delete your Cipher account, server-side messages, or remote data. Cipher will then close and
        open this device&apos;s uninstall flow.
      </p>
      {unavailable ? (
        <p className="type-caption text-destructive" role="status">
          Uninstall is available from an installed Cipher app.
        </p>
      ) : null}
      <div className="flex flex-wrap gap-2">
        <Button onClick={() => void startRemoval()} disabled={isPending} variant="destructive">
          <Trash2 aria-hidden="true" size={17} strokeWidth={1.8} />
          {isPending ? "Closing Cipher…" : "Uninstall now"}
        </Button>
        <Button onClick={() => setConfirming(false)} disabled={isPending} variant="secondary">
          Cancel
        </Button>
      </div>
    </div>
  );
}

/** Non-secret device controls that remain useful before communications features ship. */
export function DeviceSettings(): JSX.Element {
  return (
    <Card className="p-panel">
      <h2 className="type-section-title">Device</h2>
      <div className="mt-panel grid gap-2">
        <PlaceholderSetting icon={Mic} title="Voice" />
        <PlaceholderSetting icon={Video} title="Video" />
        <PlaceholderSetting icon={Bell} title="Notifications" />
      </div>
    </Card>
  );
}

/** The project-owned changelog, rendered from its checked-in Markdown source. */
export function ChangelogSettings(): JSX.Element {
  return (
    <Card className="p-panel">
      <div className="flex items-center gap-3">
        <Newspaper aria-hidden="true" className="shrink-0 text-muted" size={18} strokeWidth={1.8} />
        <h2 className="type-section-title">Changelog</h2>
      </div>
      <div className="mt-panel max-h-[min(34rem,calc(100dvh-15rem))] overflow-auto rounded-md bg-elevated p-3 text-text">
        {renderChangelog()}
      </div>
    </Card>
  );
}

/** Native uninstall and credential-removal controls kept separate from device preferences. */
export function UninstallSettings(): JSX.Element {
  return (
    <Card className="flex min-h-52 flex-col p-panel">
      <div>
        <h2 className="type-section-title">Uninstall Cipher</h2>
        <p className="type-body-muted mt-paragraph max-w-prose">
          Remove Cipher from this device and choose whether saved credentials stay available for a
          future reinstall.
        </p>
      </div>
      <div className="mt-auto pt-panel">
        <UninstallCipher />
      </div>
    </Card>
  );
}

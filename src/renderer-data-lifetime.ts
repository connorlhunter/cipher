/**
 * Renderer-only limits for display data that has already been prepared by the
 * native core. These limits deliberately sit below the protocol transport
 * limits: the webview receives only the current screen, not message history.
 */
export const rendererDataLimits = {
  maxAccountIdLength: 40,
  maxConversationIdLength: 40,
  maxMessageIdLength: 40,
  maxConversationTitleBytes: 120,
  maxMessageAuthorBytes: 80,
  maxMessagePreviewBytes: 512,
  maxVisibleMessages: 50,
  maxViewAgeMs: 60_000,
} as const;

/** Lifecycle transitions that immediately clear renderer-owned data. */
export const rendererPurgeReasons = [
  "logout",
  "device-revoked",
  "app-locked",
  "account-changed",
] as const;

/** A lifecycle transition that invalidates all renderer-owned display data. */
export type RendererPurgeReason = (typeof rendererPurgeReasons)[number];

/**
 * Native event names reserved for lifecycle work. Their payload is ignored so
 * an event cannot introduce account or message data into the webview.
 */
export const rendererPurgeEvents = Object.freeze({
  logout: "cipher://renderer-data/logout",
  "device-revoked": "cipher://renderer-data/device-revoked",
  "app-locked": "cipher://renderer-data/app-locked",
  "account-changed": "cipher://renderer-data/account-changed",
} satisfies Record<RendererPurgeReason, string>);

/** A bounded message preview for the visible conversation only. */
export interface RendererMessageView {
  id: string;
  author: string;
  preview: string;
  receivedAt: string;
}

/** The only plaintext-bearing view shape retained by the renderer. */
export interface RendererConversationView {
  accountId: string;
  conversationId: string;
  conversationTitle: string;
  messages: readonly RendererMessageView[];
}

/** The generic, content-free notification shown for a newly available message. */
export const genericMessageNotification = Object.freeze({
  title: "Cipher",
  body: "You have a new message.",
});

/** The only renderer failure shape suitable for a later diagnostics exporter. */
export const redactedRendererFailure = Object.freeze({
  code: "renderer_failure",
  message: "Cipher could not display the current screen.",
});

const identifierSuffixLength = 36;
const canonicalTimestampLength = 24;
const invalidRendererViewMessage = "The native core returned an invalid renderer view.";

type UnknownRecord = Record<string, unknown>;

/** A callback returned by a lifecycle event subscription. */
export type StopRendererPurgeListener = () => void | Promise<void>;

/** A narrow abstraction over the native event subscription API. */
export type RendererPurgeEventSubscriber = (
  eventName: string,
  handler: () => void,
) => Promise<StopRendererPurgeListener>;

/** A narrow clipboard writer. Renderer code never reads clipboard contents. */
export interface RendererClipboard {
  writeText(value: string): Promise<void>;
}

/** Browser persistence cleanup used at startup and on each lifecycle purge. */
export interface RendererPersistence {
  clear(): Promise<void>;
}

/** Browser APIs needed to remove renderer-retained state. */
export interface RendererBrowserStorageHost {
  localStorage: Pick<Storage, "clear">;
  sessionStorage: Pick<Storage, "clear">;
  indexedDB?: Pick<IDBFactory, "deleteDatabase"> & {
    databases?: () => Promise<readonly IDBDatabaseInfo[]>;
  };
  caches?: Pick<CacheStorage, "delete" | "keys">;
}

/**
 * Validates and copies one conversation view received from the native core.
 * Unknown fields are rejected so credentials, encrypted state, and arbitrary
 * transport responses cannot become renderer state by accident.
 */
export function parseRendererConversation(value: unknown): RendererConversationView {
  const view = requiredRecord(value);
  assertExactKeys(view, ["accountId", "conversationId", "conversationTitle", "messages"]);

  const accountId = requiredIdentifier(
    view.accountId,
    "usr",
    rendererDataLimits.maxAccountIdLength,
  );
  const conversationId = requiredIdentifier(
    view.conversationId,
    "cnv",
    rendererDataLimits.maxConversationIdLength,
  );
  const conversationTitle = requiredDisplayText(
    view.conversationTitle,
    rendererDataLimits.maxConversationTitleBytes,
  );

  if (
    !Array.isArray(view.messages) ||
    view.messages.length > rendererDataLimits.maxVisibleMessages
  ) {
    throw new Error(invalidRendererViewMessage);
  }

  const messages = view.messages.map(parseRendererMessage);
  return freezeConversation({ accountId, conversationId, conversationTitle, messages });
}

/**
 * Retains exactly one renderer view for a short interval. It has no persistence
 * adapter and does not expose mutable message collections.
 */
export class EphemeralRendererCache {
  private current: RendererConversationView | null = null;
  private storedAt = 0;

  public constructor(
    private readonly now: () => number = Date.now,
    private readonly maxAgeMs: number = rendererDataLimits.maxViewAgeMs,
  ) {}

  /** Stores a validated current-screen view and discards the previous one. */
  public replace(value: unknown): RendererConversationView {
    const conversation = parseRendererConversation(value);
    this.current = conversation;
    this.storedAt = this.now();
    return conversation;
  }

  /** Returns the current view until it expires, then removes it. */
  public read(): RendererConversationView | null {
    if (this.current === null) {
      return null;
    }

    const elapsed = this.now() - this.storedAt;
    if (elapsed < 0 || elapsed >= this.maxAgeMs) {
      this.clear();
      return null;
    }

    return this.current;
  }

  /** Drops the in-memory screen view without retaining a tombstone. */
  public clear(): void {
    this.current = null;
    this.storedAt = 0;
  }
}

/**
 * Coordinates the in-memory cache with best-effort cleanup of browser storage
 * and the system clipboard. Cleanup failures are intentionally not logged or
 * rethrown because error payloads can contain display data.
 */
export class RendererDataLifetime {
  public constructor(
    private readonly cache: EphemeralRendererCache,
    private readonly persistence: RendererPersistence,
    private readonly clipboard?: RendererClipboard,
  ) {}

  /** Replaces the visible conversation, purging before an account transition. */
  public async replaceConversation(value: unknown): Promise<RendererConversationView> {
    const next = parseRendererConversation(value);
    const current = this.cache.read();

    if (current !== null && current.accountId !== next.accountId) {
      await this.purge("account-changed");
    }

    return this.cache.replace(next);
  }

  /** Returns the currently valid display view, if any. */
  public currentConversation(): RendererConversationView | null {
    return this.cache.read();
  }

  /** Clears the current view and all supported browser-retained data. */
  public async clear(): Promise<void> {
    this.cache.clear();
    const clipboard = this.clipboard;
    await Promise.allSettled([
      Promise.resolve().then(() => this.persistence.clear()),
      clipboard === undefined
        ? Promise.resolve()
        : Promise.resolve().then(() => clearClipboard(clipboard)),
    ]);
  }

  /** Clears renderer data after a security- or account-related lifecycle transition. */
  public async purge(_reason: RendererPurgeReason): Promise<void> {
    await this.clear();
  }
}

/** Creates the browser-backed data lifetime used by the desktop shell. */
export function createBrowserRendererDataLifetime(
  storageHost: RendererBrowserStorageHost = window,
  clipboard: RendererClipboard | undefined = navigator.clipboard,
): RendererDataLifetime {
  return new RendererDataLifetime(
    new EphemeralRendererCache(),
    browserRendererPersistence(storageHost),
    clipboard,
  );
}

/**
 * Copies only a validated, bounded preview in response to an explicit user
 * action. The caller must invoke this from that action; no view is copied
 * automatically.
 */
export async function copyRendererPreview(
  preview: unknown,
  clipboard: RendererClipboard,
): Promise<boolean> {
  try {
    await clipboard.writeText(
      requiredDisplayText(preview, rendererDataLimits.maxMessagePreviewBytes),
    );
    return true;
  } catch {
    return false;
  }
}

/**
 * Subscribes to no-payload native lifecycle events and purges state for each
 * transition. If setup is interrupted, already-registered listeners are
 * removed before the failure is reported to the caller.
 */
export async function subscribeToRendererPurgeEvents(
  subscribe: RendererPurgeEventSubscriber,
  purge: (reason: RendererPurgeReason) => Promise<void>,
): Promise<() => Promise<void>> {
  const stops: StopRendererPurgeListener[] = [];

  try {
    for (const reason of rendererPurgeReasons) {
      stops.push(
        await subscribe(rendererPurgeEvents[reason], () => {
          void purge(reason);
        }),
      );
    }
  } catch {
    await Promise.allSettled(stops.map(async (stop) => stop()));
    throw new Error("Cipher could not subscribe to lifecycle cleanup events.");
  }

  return async () => {
    await Promise.allSettled(stops.map(async (stop) => stop()));
  };
}

/** Creates a best-effort browser persistence cleaner without reading stored values. */
export function browserRendererPersistence(host: RendererBrowserStorageHost): RendererPersistence {
  return {
    async clear(): Promise<void> {
      clearWebStorage(host.localStorage);
      clearWebStorage(host.sessionStorage);
      await Promise.all([clearIndexedDatabases(host.indexedDB), clearCacheStorage(host.caches)]);
    },
  };
}

function parseRendererMessage(value: unknown): RendererMessageView {
  const message = requiredRecord(value);
  assertExactKeys(message, ["id", "author", "preview", "receivedAt"]);

  return Object.freeze({
    id: requiredIdentifier(message.id, "msg", rendererDataLimits.maxMessageIdLength),
    author: requiredDisplayText(message.author, rendererDataLimits.maxMessageAuthorBytes),
    preview: requiredDisplayText(message.preview, rendererDataLimits.maxMessagePreviewBytes),
    receivedAt: requiredTimestamp(message.receivedAt),
  });
}

function freezeConversation(value: RendererConversationView): RendererConversationView {
  return Object.freeze({ ...value, messages: Object.freeze([...value.messages]) });
}

function requiredRecord(value: unknown): UnknownRecord {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error(invalidRendererViewMessage);
  }

  return value as UnknownRecord;
}

function assertExactKeys(value: UnknownRecord, expectedKeys: readonly string[]): void {
  const keys = Object.keys(value);
  if (
    keys.length !== expectedKeys.length ||
    expectedKeys.some((expectedKey) => !Object.hasOwn(value, expectedKey))
  ) {
    throw new Error(invalidRendererViewMessage);
  }
}

function requiredIdentifier(value: unknown, prefix: string, maximumLength: number): string {
  if (
    typeof value !== "string" ||
    value.length !== maximumLength ||
    !value.startsWith(`${prefix}_`) ||
    !isCanonicalUuidV7(value.slice(prefix.length + 1))
  ) {
    throw new Error(invalidRendererViewMessage);
  }

  return value;
}

function isCanonicalUuidV7(value: string): boolean {
  if (value.length !== identifierSuffixLength) {
    return false;
  }

  for (const [index, character] of Array.from(value).entries()) {
    if ([8, 13, 18, 23].includes(index)) {
      if (character !== "-") {
        return false;
      }
    } else if (index === 14) {
      if (character !== "7") {
        return false;
      }
    } else if (index === 19) {
      if (!"89ab".includes(character)) {
        return false;
      }
    } else if (!"0123456789abcdef".includes(character)) {
      return false;
    }
  }

  return true;
}

function requiredDisplayText(value: unknown, maximumBytes: number): string {
  if (
    typeof value !== "string" ||
    value.length === 0 ||
    new TextEncoder().encode(value).byteLength > maximumBytes ||
    hasDisplayControlCharacter(value)
  ) {
    throw new Error(invalidRendererViewMessage);
  }

  return value;
}

function hasDisplayControlCharacter(value: string): boolean {
  return Array.from(value).some((character) => {
    const codePoint = character.codePointAt(0);
    return codePoint !== undefined && (codePoint <= 0x1f || codePoint === 0x7f);
  });
}

function requiredTimestamp(value: unknown): string {
  if (
    typeof value !== "string" ||
    value.length !== canonicalTimestampLength ||
    !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/u.test(value) ||
    Number.isNaN(Date.parse(value)) ||
    new Date(value).toISOString() !== value
  ) {
    throw new Error(invalidRendererViewMessage);
  }

  return value;
}

function clearWebStorage(storage: Pick<Storage, "clear">): void {
  try {
    storage.clear();
  } catch {
    // Storage can be disabled by the platform. Retained values are never read or logged.
  }
}

async function clearIndexedDatabases(
  indexedDB: RendererBrowserStorageHost["indexedDB"],
): Promise<void> {
  if (indexedDB?.databases === undefined) {
    return;
  }

  try {
    const databases = await indexedDB.databases();
    await Promise.all(
      databases.flatMap(({ name }) =>
        name === undefined ? [] : [deleteIndexedDatabase(indexedDB, name)],
      ),
    );
  } catch {
    // Enumeration may be unavailable in an older WebView. No values are inspected or logged.
  }
}

function deleteIndexedDatabase(
  indexedDB: NonNullable<RendererBrowserStorageHost["indexedDB"]>,
  name: string,
): Promise<void> {
  return new Promise((resolve) => {
    try {
      const request = indexedDB.deleteDatabase(name);
      const finish = (): void => resolve();
      request.addEventListener("success", finish, { once: true });
      request.addEventListener("error", finish, { once: true });
      request.addEventListener("blocked", finish, { once: true });
    } catch {
      resolve();
    }
  });
}

async function clearCacheStorage(caches: RendererBrowserStorageHost["caches"]): Promise<void> {
  if (caches === undefined) {
    return;
  }

  try {
    const names = await caches.keys();
    await Promise.all(names.map(async (name) => caches.delete(name)));
  } catch {
    // CacheStorage may be disabled by the platform. Retained values are never read or logged.
  }
}

async function clearClipboard(clipboard: RendererClipboard): Promise<void> {
  await clipboard.writeText("");
}

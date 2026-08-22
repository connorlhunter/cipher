import { describe, expect, test } from "bun:test";
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";

import { listenForRendererPurgeEventsWith } from "../src/desktop";
import {
  EphemeralRendererCache,
  RendererDataLifetime,
  browserRendererPersistence,
  copyRendererPreview,
  createBrowserRendererDataLifetime,
  genericMessageNotification,
  parseRendererConversation,
  redactedRendererFailure,
  rendererDataLimits,
  rendererPurgeEvents,
  rendererPurgeReasons,
  subscribeToRendererPurgeEvents,
  type RendererBrowserStorageHost,
  type RendererClipboard,
  type RendererPurgeReason,
} from "../src/renderer-data-lifetime";

const accountId = "usr_018f9a76-4c00-7a12-8b0c-4d5e6f708192";
const otherAccountId = "usr_018f9a76-4c01-7a12-8b0c-4d5e6f708192";
const conversationId = "cnv_018f9a76-4c03-7a12-8b0c-4d5e6f708192";
const messageId = "msg_018f9a76-4c06-7a12-8b0c-4d5e6f708192";

function conversation(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    accountId,
    conversationId,
    conversationTitle: "Cipher planning",
    messages: [
      {
        id: messageId,
        author: "Casey",
        preview: "The current screen is bounded.",
        receivedAt: "2026-08-21T18:42:15.123Z",
      },
    ],
    ...overrides,
  };
}

function clipboard(writes: string[], fails = false): RendererClipboard {
  return {
    async writeText(value: string): Promise<void> {
      if (fails) {
        throw new Error("clipboard unavailable");
      }
      writes.push(value);
    },
  };
}

describe("parseRendererConversation", () => {
  test("accepts a frozen, bounded current-screen view", () => {
    const parsed = parseRendererConversation(conversation());

    expect(parsed.accountId).toBe(accountId);
    expect(parsed.conversationId).toBe(conversationId);
    expect(parsed.messages[0]?.preview).toBe("The current screen is bounded.");
    expect(Object.isFrozen(parsed)).toBe(true);
    expect(Object.isFrozen(parsed.messages)).toBe(true);
    expect(Object.isFrozen(parsed.messages[0])).toBe(true);
  });

  const unsafeViews: readonly [string, unknown][] = [
    ["a non-object value", null],
    ["an incomplete object", {}],
    ["an unknown field", conversation({ token: "not allowed" })],
    ["a non-canonical account ID", conversation({ accountId: "usr_not-a-canonical-id" })],
    [
      "control characters in a preview",
      conversation({
        messages: [
          {
            id: messageId,
            author: "Casey",
            preview: "line one\nline two",
            receivedAt: "2026-08-21T18:42:15.123Z",
          },
        ],
      }),
    ],
    [
      "an invalid timestamp",
      conversation({
        messages: [
          {
            id: messageId,
            author: "Casey",
            preview: "The current screen is bounded.",
            receivedAt: "2026-02-30T18:42:15.123Z",
          },
        ],
      }),
    ],
    [
      "too many visible messages",
      conversation({
        messages: Array.from({ length: rendererDataLimits.maxVisibleMessages + 1 }, () => ({
          id: messageId,
          author: "Casey",
          preview: "The current screen is bounded.",
          receivedAt: "2026-08-21T18:42:15.123Z",
        })),
      }),
    ],
  ];

  for (const [description, value] of unsafeViews) {
    test(`rejects ${description}`, () => {
      expect(() => parseRendererConversation(value)).toThrow("invalid renderer view");
    });
  }
});

describe("EphemeralRendererCache", () => {
  test("keeps one view for a short interval and clears expired or clock-reversed data", () => {
    let now = 100;
    const cache = new EphemeralRendererCache(() => now, 60);

    expect(cache.read()).toBeNull();
    cache.replace(conversation());
    expect(cache.read()?.accountId).toBe(accountId);

    now = 159;
    expect(cache.read()?.conversationId).toBe(conversationId);

    now = 160;
    expect(cache.read()).toBeNull();

    cache.replace(conversation());
    now = 99;
    expect(cache.read()).toBeNull();
  });
});

describe("RendererDataLifetime", () => {
  test("purges the previous account before retaining the next one", async () => {
    const writes: string[] = [];
    let persistenceClears = 0;
    const lifetime = new RendererDataLifetime(
      new EphemeralRendererCache(),
      {
        async clear(): Promise<void> {
          persistenceClears += 1;
        },
      },
      clipboard(writes),
    );

    await lifetime.replaceConversation(conversation());
    await lifetime.replaceConversation(conversation({ accountId: otherAccountId }));

    expect(persistenceClears).toBe(1);
    expect(writes).toEqual([""]);
    expect(lifetime.currentConversation()?.accountId).toBe(otherAccountId);
  });

  for (const reason of rendererPurgeReasons) {
    test(`clears data for ${reason}`, async () => {
      const writes: string[] = [];
      let persistenceClears = 0;
      const lifetime = new RendererDataLifetime(
        new EphemeralRendererCache(),
        {
          async clear(): Promise<void> {
            persistenceClears += 1;
          },
        },
        clipboard(writes),
      );

      await lifetime.replaceConversation(conversation());
      await lifetime.purge(reason);

      expect(lifetime.currentConversation()).toBeNull();
      expect(persistenceClears).toBe(1);
      expect(writes).toEqual([""]);
    });
  }

  test("does not expose cleanup errors to the renderer", async () => {
    const lifetime = new RendererDataLifetime(
      new EphemeralRendererCache(),
      {
        clear(): Promise<void> {
          throw new Error("persistent value");
        },
      },
      {
        writeText(): Promise<void> {
          throw new Error("clipboard unavailable");
        },
      },
    );

    await lifetime.replaceConversation(conversation());
    await expect(lifetime.clear()).resolves.toBeUndefined();
    expect(lifetime.currentConversation()).toBeNull();
  });
});

describe("browser renderer persistence", () => {
  test("clears web storage, indexed databases, and CacheStorage without reading values", async () => {
    const calls = {
      local: 0,
      session: 0,
      databases: [] as string[],
      caches: [] as string[],
    };
    const host: RendererBrowserStorageHost = {
      localStorage: { clear: () => void (calls.local += 1) },
      sessionStorage: { clear: () => void (calls.session += 1) },
      indexedDB: {
        async databases(): Promise<readonly IDBDatabaseInfo[]> {
          return [{ name: "cipher-renderer" }, {}];
        },
        deleteDatabase(name: string): IDBOpenDBRequest {
          calls.databases.push(name);
          return {
            addEventListener(eventName: string, listener: () => void): void {
              if (eventName === "success") {
                queueMicrotask(listener);
              }
            },
          } as unknown as IDBOpenDBRequest;
        },
      },
      caches: {
        async keys(): Promise<string[]> {
          return ["cipher-renderer"];
        },
        async delete(name: string): Promise<boolean> {
          calls.caches.push(name);
          return true;
        },
      },
    };

    await browserRendererPersistence(host).clear();

    expect(calls).toEqual({
      local: 1,
      session: 1,
      databases: ["cipher-renderer"],
      caches: ["cipher-renderer"],
    });
  });

  test("continues when storage cleanup is unavailable", async () => {
    const host: RendererBrowserStorageHost = {
      localStorage: {
        clear(): void {
          throw new Error("local storage disabled");
        },
      },
      sessionStorage: {
        clear(): void {
          throw new Error("session storage disabled");
        },
      },
      indexedDB: {
        async databases(): Promise<readonly IDBDatabaseInfo[]> {
          throw new Error("indexed database disabled");
        },
        deleteDatabase(): IDBOpenDBRequest {
          throw new Error("unreachable");
        },
      },
      caches: {
        async keys(): Promise<string[]> {
          throw new Error("cache storage disabled");
        },
        async delete(): Promise<boolean> {
          return false;
        },
      },
    };

    await expect(browserRendererPersistence(host).clear()).resolves.toBeUndefined();
    await expect(
      browserRendererPersistence({
        localStorage: { clear: (): void => undefined },
        sessionStorage: { clear: (): void => undefined },
      }).clear(),
    ).resolves.toBeUndefined();
  });

  test("builds a browser-backed lifetime from injected browser services", async () => {
    const writes: string[] = [];
    const lifetime = createBrowserRendererDataLifetime(
      {
        localStorage: { clear: (): void => undefined },
        sessionStorage: { clear: (): void => undefined },
      },
      clipboard(writes),
    );

    await lifetime.clear();
    expect(writes).toEqual([""]);
  });
});

describe("clipboard, notification, and diagnostics policy", () => {
  test("copies only bounded visible text and hides failed writes", async () => {
    const writes: string[] = [];

    await expect(copyRendererPreview("Copy this preview", clipboard(writes))).resolves.toBe(true);
    await expect(copyRendererPreview("", clipboard(writes))).resolves.toBe(false);
    await expect(
      copyRendererPreview(
        "x".repeat(rendererDataLimits.maxMessagePreviewBytes + 1),
        clipboard(writes),
      ),
    ).resolves.toBe(false);
    await expect(copyRendererPreview("Copy this preview", clipboard([], true))).resolves.toBe(
      false,
    );
    expect(writes).toEqual(["Copy this preview"]);
  });

  test("uses content-free notifications and diagnostics without renderer logging", () => {
    expect(genericMessageNotification).toEqual({
      title: "Cipher",
      body: "You have a new message.",
    });
    expect(redactedRendererFailure).toEqual({
      code: "renderer_failure",
      message: "Cipher could not display the current screen.",
    });

    const rendererSource = readdirSync("src", { recursive: true })
      .filter((path): path is string => typeof path === "string" && /\.tsx?$/u.test(path))
      .map((path) => readFileSync(join("src", path), "utf8"))
      .join("\n");
    expect(rendererSource).not.toContain("console.");
    expect(rendererSource).not.toMatch(/(?:localStorage|sessionStorage)\.(?:getItem|setItem)/u);
    expect(rendererSource).not.toContain("indexedDB.open");
    expect(rendererSource).not.toContain("caches.open");
    expect(rendererSource).not.toContain("document.cookie");
  });
});

describe("renderer lifecycle subscriptions", () => {
  test("purges for every no-payload lifecycle event and removes listeners", async () => {
    const handlers = new Map<string, () => void>();
    const stopped: string[] = [];
    const purged: RendererPurgeReason[] = [];
    let resolvePurge: (() => void) | undefined;
    const purgeFinished = new Promise<void>((resolve) => {
      resolvePurge = resolve;
    });

    const stop = await subscribeToRendererPurgeEvents(
      async (eventName, handler) => {
        handlers.set(eventName, handler);
        return () => {
          stopped.push(eventName);
        };
      },
      async (reason) => {
        purged.push(reason);
        resolvePurge?.();
      },
    );

    expect([...handlers.keys()]).toEqual(Object.values(rendererPurgeEvents));
    handlers.get(rendererPurgeEvents["device-revoked"])?.();
    await purgeFinished;
    expect(purged).toEqual(["device-revoked"]);

    await stop();
    expect(stopped).toEqual(Object.values(rendererPurgeEvents));
  });

  test("removes earlier listeners when registration fails", async () => {
    let registrations = 0;
    let stopped = 0;

    await expect(
      subscribeToRendererPurgeEvents(
        async () => {
          registrations += 1;
          if (registrations === 2) {
            throw new Error("native event unavailable");
          }
          return () => {
            stopped += 1;
          };
        },
        async () => undefined,
      ),
    ).rejects.toThrow("could not subscribe");

    expect(stopped).toBe(1);
  });

  test("keeps the desktop event bridge injectable", async () => {
    const events: string[] = [];
    const stop = await listenForRendererPurgeEventsWith(
      async (eventName) => {
        events.push(eventName);
        return () => undefined;
      },
      async () => undefined,
    );

    expect(events).toEqual(Object.values(rendererPurgeEvents));
    await stop();
  });
});

/**
 * The current native-to-webview contract version.
 *
 * Version zero is accepted only while clients update to version one. New
 * desktop commands must use the current version.
 */
export const desktopProtocol = {
  current: 1,
  previous: 0,
} as const;

/** Commands the webview is permitted to invoke in this release. */
export const desktopCommands = {
  status: "desktop_status",
} as const;

/** The largest display-only status message accepted from the native core. */
export const maxDesktopStatusMessageLength = 160;

/** A bounded, display-only native-core status view. */
export interface DesktopStatus {
  message: string;
}

/** Lifecycle events that may be sent from Rust to the webview. */
export type DesktopLifecycleEvent =
  { kind: "ready"; protocolVersion: number } | { kind: "shutdown"; protocolVersion: number };

/** Typed error codes that can cross the native boundary. */
export type DesktopIpcErrorCode =
  "cancelled" | "invalid_request" | "unsupported_version" | "unavailable";

/** A safe, bounded error shape for native command failures. */
export interface DesktopIpcError {
  code: DesktopIpcErrorCode;
  message: string;
}

/**
 * Validates a response view before it reaches React.
 *
 * Tokens, private keys, serialized MLS state, and arbitrary plaintext are
 * deliberately absent from every desktop view model.
 */
export function parseDesktopStatus(value: unknown): DesktopStatus {
  if (
    typeof value !== "object" ||
    value === null ||
    !("message" in value) ||
    typeof value.message !== "string" ||
    value.message.length === 0 ||
    value.message.length > maxDesktopStatusMessageLength ||
    Object.keys(value).some((key) => /(?:token|secret|key|mls|plaintext)/iu.test(key))
  ) {
    throw new Error("The desktop core returned an invalid status.");
  }

  return { message: value.message };
}

/** Returns whether a desktop protocol version is temporarily compatible. */
export function supportsDesktopProtocol(version: number): boolean {
  return version === desktopProtocol.current || version === desktopProtocol.previous;
}

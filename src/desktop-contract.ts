import { z } from "zod";

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
  diagnostics: "desktop_diagnostics",
  theme: "desktop_theme",
  setTheme: "desktop_set_theme",
  authenticate: "desktop_authenticate",
  removeCipher: "desktop_remove_cipher",
} as const;

/** The largest display-only status message accepted from the native core. */
export const maxDesktopStatusMessageLength = 160;

/** A bounded, display-only native-core status view. */
export interface DesktopStatus {
  message: string;
}

/** A bounded result of one native-owned authentication submission. */
export interface DesktopAuthenticationView {
  state:
    | "authenticated"
    | "challenge_required"
    | "password_reset_required"
    | "password_reset_complete"
    | "failed";
  message: string;
}

/** A bounded acknowledgement that the native platform removal flow has started. */
export interface DesktopRemovalView {
  message: string;
}

/** The native lifecycle states safe to show in a desktop diagnostics view. */
export const desktopLifecycleStates = [
  "starting",
  "active",
  "locked",
  "sleeping",
  "offline",
  "shutting_down",
  "stopped",
] as const;

/** The native transport states safe to show in a desktop diagnostics view. */
export const nativeTransportStates = ["ready", "paused", "offline"] as const;

/** A bounded, content-free desktop diagnostics view. */
export interface DesktopDiagnostics {
  lifecycleState: (typeof desktopLifecycleStates)[number];
  transportState: (typeof nativeTransportStates)[number];
  rendererEpoch: number;
  activeOperations: number;
  coldStarts: number;
  wakes: number;
}

/** The bounded color schemes supported by the desktop design system. */
export const desktopThemeSchemes = [
  "atlas",
  "paper",
  "citrine",
  "harbor",
  "midnight",
  "onyx",
  "rose",
  "tide",
  "ember",
  "quartz",
] as const;

/** The one application-wide system or explicit-scheme preference owned by native code. */
export const desktopThemePreferences = ["system", ...desktopThemeSchemes] as const;

/** A concrete appearance resolved by the native window manager. */
export const resolvedDesktopThemes = ["light", "dark"] as const;

export type DesktopThemeScheme = (typeof desktopThemeSchemes)[number];
export type DesktopThemePreference = (typeof desktopThemePreferences)[number];
export type ResolvedDesktopTheme = (typeof resolvedDesktopThemes)[number];

/** Native window classification for every explicit scheme. */
export const desktopThemeSchemeClassifications = Object.freeze({
  atlas: "light",
  paper: "light",
  citrine: "light",
  harbor: "dark",
  midnight: "dark",
  onyx: "dark",
  rose: "light",
  tide: "light",
  ember: "light",
  quartz: "light",
} satisfies Record<DesktopThemeScheme, ResolvedDesktopTheme>);

/** A safe, content-free theme view supplied by the native desktop core. */
export interface DesktopTheme {
  preference: DesktopThemePreference;
  scheme: DesktopThemeScheme;
  resolved: ResolvedDesktopTheme;
}

/** A no-payload signal that asks the webview to re-read the native theme view. */
export const desktopThemeChangedEvent = "cipher://theme/changed";

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

/** Validates a display-only authentication result before React renders it. */
export function parseDesktopAuthenticationView(value: unknown): DesktopAuthenticationView {
  const result = z
    .object({
      state: z.enum([
        "authenticated",
        "challenge_required",
        "password_reset_required",
        "password_reset_complete",
        "failed",
      ]),
      message: z.string().min(1).max(maxDesktopStatusMessageLength),
    })
    .strict()
    .safeParse(value);
  if (!result.success) {
    throw new Error("The desktop core returned an invalid authentication result.");
  }
  return Object.freeze({ ...result.data });
}

/** Validates a display-safe native removal acknowledgement before React renders it. */
export function parseDesktopRemovalView(value: unknown): DesktopRemovalView {
  const result = z
    .object({
      message: z.string().min(1).max(maxDesktopStatusMessageLength),
    })
    .strict()
    .safeParse(value);
  if (!result.success) {
    throw new Error("The desktop core returned an invalid removal result.");
  }
  return Object.freeze({ ...result.data });
}

/** Validates a bounded diagnostic export without accepting arbitrary native state. */
export function parseDesktopDiagnostics(value: unknown): DesktopDiagnostics {
  if (
    typeof value !== "object" ||
    value === null ||
    Object.keys(value).length !== 6 ||
    !(
      "lifecycleState" in value &&
      "transportState" in value &&
      "rendererEpoch" in value &&
      "activeOperations" in value &&
      "coldStarts" in value &&
      "wakes" in value
    ) ||
    !desktopLifecycleStates.includes(
      value.lifecycleState as (typeof desktopLifecycleStates)[number],
    ) ||
    !nativeTransportStates.includes(
      value.transportState as (typeof nativeTransportStates)[number],
    ) ||
    !isSafeCounter(value.rendererEpoch) ||
    !isSafeCounter(value.activeOperations) ||
    value.activeOperations > 32 ||
    !isSafeCounter(value.coldStarts) ||
    !isSafeCounter(value.wakes)
  ) {
    throw new Error("The desktop core returned invalid diagnostics.");
  }

  return {
    lifecycleState: value.lifecycleState as DesktopDiagnostics["lifecycleState"],
    transportState: value.transportState as DesktopDiagnostics["transportState"],
    rendererEpoch: value.rendererEpoch,
    activeOperations: value.activeOperations,
    coldStarts: value.coldStarts,
    wakes: value.wakes,
  };
}

/** Validates the native-owned, resolved theme before it reaches the application shell. */
export function parseDesktopTheme(value: unknown): DesktopTheme {
  const result = z
    .object({
      preference: z.enum(desktopThemePreferences),
      scheme: z.enum(desktopThemeSchemes),
      resolved: z.enum(resolvedDesktopThemes),
    })
    .strict()
    .refine(
      (theme) =>
        desktopThemeSchemeClassifications[theme.scheme] === theme.resolved &&
        (theme.preference === "system"
          ? (theme.resolved === "light" && theme.scheme === "atlas") ||
            (theme.resolved === "dark" && theme.scheme === "midnight")
          : theme.preference === theme.scheme),
      "The desktop core returned a contradictory theme.",
    )
    .safeParse(value);

  if (!result.success) {
    throw new Error("The desktop core returned an invalid theme.");
  }

  return Object.freeze({ ...result.data });
}

/** Returns whether a desktop protocol version is temporarily compatible. */
export function supportsDesktopProtocol(version: number): boolean {
  return version === desktopProtocol.current || version === desktopProtocol.previous;
}

function isSafeCounter(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

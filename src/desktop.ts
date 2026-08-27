import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  desktopCommands,
  desktopThemeChangedEvent,
  desktopProtocol,
  parseDesktopAuthenticationView,
  parseDesktopRemovalView,
  parseDesktopDiagnostics,
  parseDesktopStatus,
  parseDesktopTheme,
  type DesktopDiagnostics,
  type DesktopAuthenticationView,
  type DesktopRemovalView,
  type DesktopStatus,
  type DesktopTheme,
  type DesktopThemePreference,
} from "./desktop-contract";
import {
  subscribeToRendererPurgeEvents,
  type RendererPurgeEventSubscriber,
  type RendererPurgeReason,
} from "./renderer-data-lifetime";

export {
  parseDesktopDiagnostics,
  parseDesktopAuthenticationView,
  parseDesktopRemovalView,
  parseDesktopStatus,
  parseDesktopTheme,
  type DesktopDiagnostics,
  type DesktopAuthenticationView,
  type DesktopRemovalView,
  type DesktopStatus,
  type DesktopTheme,
  type DesktopThemePreference,
} from "./desktop-contract";

export type { DesktopThemeScheme } from "./desktop-contract";

/** One-time credentials accepted only by the native authentication command. */
export type DesktopAuthenticationRequest =
  | { flow: "sign_in"; identifier: string; password: string }
  | { flow: "begin_password_reset"; identifier: string }
  | { flow: "confirm_password_reset"; identifier: string; code: string; newPassword: string }
  | { flow: "continue_challenge"; code: string }
  | {
      flow: "accept_administrator_invitation";
      identifier: string;
      temporaryPassword: string;
      newPassword: string;
    };

/** Invokes the one-time native credential boundary and accepts only a safe response view. */
export async function desktopAuthenticateWith(
  invokeCommand: (command: string, arguments_: Record<string, unknown>) => Promise<unknown>,
  request: DesktopAuthenticationRequest,
): Promise<DesktopAuthenticationView> {
  return parseDesktopAuthenticationView(
    await invokeCommand(desktopCommands.authenticate, {
      request,
      protocolVersion: desktopProtocol.current,
    }),
  );
}

/** Submits credentials directly to native handling without persisting them in the webview. */
export async function desktopAuthenticate(
  request: DesktopAuthenticationRequest,
): Promise<DesktopAuthenticationView> {
  return desktopAuthenticateWith(
    (command, arguments_) => invoke<unknown>(command, arguments_),
    request,
  );
}

/** Starts the native removal handoff without exposing platform paths to the webview. */
export async function desktopRemoveCipher(removeLocalData: boolean): Promise<DesktopRemovalView> {
  return parseDesktopRemovalView(
    await invoke<unknown>(desktopCommands.removeCipher, {
      request: { removeLocalData },
      protocolVersion: desktopProtocol.current,
    }),
  );
}

/** Invokes and validates the native desktop-status command. */
export async function desktopStatusWith(
  invokeCommand: (command: string, arguments_: { protocolVersion: number }) => Promise<unknown>,
): Promise<DesktopStatus> {
  return parseDesktopStatus(
    await invokeCommand(desktopCommands.status, { protocolVersion: desktopProtocol.current }),
  );
}

/**
 * @returns The validated status of the native desktop core.
 */
export async function desktopStatus(): Promise<DesktopStatus> {
  return desktopStatusWith((command, arguments_) => invoke<unknown>(command, arguments_));
}

/** Subscribes to lifecycle cleanup events through an injectable native event source. */
export async function listenForRendererPurgeEventsWith(
  subscribe: RendererPurgeEventSubscriber,
  purge: (reason: RendererPurgeReason) => Promise<void>,
): Promise<() => Promise<void>> {
  return subscribeToRendererPurgeEvents(subscribe, purge);
}

/** Subscribes to the native lifecycle cleanup events emitted by a desktop build. */
export async function listenForRendererPurgeEvents(
  purge: (reason: RendererPurgeReason) => Promise<void>,
): Promise<() => Promise<void>> {
  return listenForRendererPurgeEventsWith(
    (eventName, handler) => listen(eventName, () => handler()),
    purge,
  );
}

/** Invokes and validates the native desktop-diagnostics command. */
export async function desktopDiagnosticsWith(
  invokeCommand: (command: string, arguments_: { protocolVersion: number }) => Promise<unknown>,
): Promise<DesktopDiagnostics> {
  return parseDesktopDiagnostics(
    await invokeCommand(desktopCommands.diagnostics, { protocolVersion: desktopProtocol.current }),
  );
}

/**
 * @returns The validated, content-free diagnostic view from the native desktop core.
 */
export async function desktopDiagnostics(): Promise<DesktopDiagnostics> {
  return desktopDiagnosticsWith((command, arguments_) => invoke<unknown>(command, arguments_));
}

/** Invokes and validates the native theme view through an injectable command source. */
export async function desktopThemeWith(
  invokeCommand: (command: string, arguments_: { protocolVersion: number }) => Promise<unknown>,
): Promise<DesktopTheme> {
  return parseDesktopTheme(
    await invokeCommand(desktopCommands.theme, { protocolVersion: desktopProtocol.current }),
  );
}

/** Returns the resolved appearance selected by the native desktop core. */
export async function desktopTheme(): Promise<DesktopTheme> {
  return desktopThemeWith((command, arguments_) => invoke<unknown>(command, arguments_));
}

/** Invokes and validates a native-owned theme preference update. */
export async function setDesktopThemeWith(
  invokeCommand: (
    command: string,
    arguments_: { preference: DesktopThemePreference; protocolVersion: number },
  ) => Promise<unknown>,
  preference: DesktopThemePreference,
): Promise<DesktopTheme> {
  return parseDesktopTheme(
    await invokeCommand(desktopCommands.setTheme, {
      preference,
      protocolVersion: desktopProtocol.current,
    }),
  );
}

/** Updates the native preference without persisting any theme value in the webview. */
export async function setDesktopTheme(preference: DesktopThemePreference): Promise<DesktopTheme> {
  return setDesktopThemeWith(
    (command, arguments_) => invoke<unknown>(command, arguments_),
    preference,
  );
}

/** A narrow subscription boundary for content-free native theme notifications. */
export type DesktopThemeEventSubscriber = (
  eventName: string,
  handler: () => void,
) => Promise<() => void | Promise<void>>;

/** Keeps the native theme-event adapter injectable for focused UI tests. */
export async function listenForDesktopThemeChangesWith(
  subscribe: DesktopThemeEventSubscriber,
  refresh: () => Promise<void>,
): Promise<() => void | Promise<void>> {
  return subscribe(desktopThemeChangedEvent, () => {
    void refresh();
  });
}

/** Re-reads the resolved theme when the native window manager reports a change. */
export async function listenForDesktopThemeChanges(
  refresh: () => Promise<void>,
): Promise<() => void | Promise<void>> {
  return listenForDesktopThemeChangesWith(
    (eventName, handler) => listen(eventName, () => handler()),
    refresh,
  );
}

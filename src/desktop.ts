import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  desktopCommands,
  desktopProtocol,
  parseDesktopStatus,
  type DesktopStatus,
} from "./desktop-contract";
import {
  subscribeToRendererPurgeEvents,
  type RendererPurgeEventSubscriber,
  type RendererPurgeReason,
} from "./renderer-data-lifetime";

export { parseDesktopStatus, type DesktopStatus } from "./desktop-contract";

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

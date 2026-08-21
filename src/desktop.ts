import { invoke } from "@tauri-apps/api/core";
import {
  desktopCommands,
  desktopProtocol,
  parseDesktopStatus,
  type DesktopStatus,
} from "./desktop-contract";

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

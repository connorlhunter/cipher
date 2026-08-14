import { invoke } from "@tauri-apps/api/core";

/**
 * @property message - Status reported by the native desktop core.
 */
export interface DesktopStatus {
  message: string;
}

/**
 * @param value - Untrusted status payload returned by the native command.
 * @returns A validated desktop status.
 */
export function parseDesktopStatus(value: unknown): DesktopStatus {
  if (
    typeof value !== "object" ||
    value === null ||
    !("message" in value) ||
    typeof value.message !== "string" ||
    value.message.length === 0
  ) {
    throw new Error("The desktop core returned an invalid status.");
  }

  return { message: value.message };
}

/**
 * @returns The validated status of the native desktop core.
 */
export async function desktopStatus(): Promise<DesktopStatus> {
  return parseDesktopStatus(await invoke<unknown>("desktop_status"));
}

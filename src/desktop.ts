import { invoke } from "@tauri-apps/api/core";

export interface DesktopStatus {
  message: string;
}

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

export async function desktopStatus(): Promise<DesktopStatus> {
  return parseDesktopStatus(await invoke<unknown>("desktop_status"));
}

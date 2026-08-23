import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

interface DesktopCapability {
  identifier: string;
  local: boolean;
  platforms: string[];
  permissions: string[];
  remote?: unknown;
  webviews: string[];
  windows?: unknown;
}

interface DesktopConfig {
  app: {
    security: {
      csp: Record<string, string>;
      devCsp: Record<string, string>;
      freezePrototype: boolean;
      headers: Record<string, string>;
    };
    windows: Array<{
      create?: boolean;
      height?: number;
      label?: string;
      minHeight?: number;
      minWidth?: number;
      title?: string;
      width?: number;
    }>;
  };
}

const capability = JSON.parse(
  readFileSync("src-tauri/capabilities/default.json", "utf8"),
) as DesktopCapability;
const desktopConfig = JSON.parse(
  readFileSync("src-tauri/tauri.conf.json", "utf8"),
) as DesktopConfig;
const buildScript = readFileSync("src-tauri/build.rs", "utf8");
const desktopEntryPoint = readFileSync("src-tauri/src/main.rs", "utf8");
const desktopManifest = readFileSync("src-tauri/Cargo.toml", "utf8");

const productionCsp = {
  "base-uri": "'none'",
  "connect-src": "ipc: http://ipc.localhost",
  "default-src": "'self'",
  "font-src": "'self'",
  "form-action": "'none'",
  "frame-ancestors": "'none'",
  "frame-src": "'none'",
  "img-src": "'self'",
  "media-src": "'none'",
  "object-src": "'none'",
  "script-src": "'self'",
  "style-src": "'self'",
  "worker-src": "'none'",
};

describe("desktop trust-boundary configuration", () => {
  test("grants the bundled main webview only the allowlisted desktop commands", () => {
    expect(capability).toMatchObject({
      identifier: "main-webview",
      local: true,
      platforms: ["macOS", "windows"],
      permissions: [
        "allow-desktop-status",
        "allow-desktop-diagnostics",
        "allow-desktop-theme",
        "allow-desktop-set-theme",
        "allow-desktop-authenticate",
      ],
      webviews: ["main"],
    });
    expect(capability.remote).toBeUndefined();
    expect(capability.windows).toBeUndefined();
    expect(buildScript).toContain('"desktop_theme"');
    expect(buildScript).toContain('"desktop_set_theme"');
    expect(buildScript).toContain('"desktop_authenticate"');
  });

  test("does not expose file, shell, or external-opening permissions", () => {
    expect(capability.permissions).toEqual([
      "allow-desktop-status",
      "allow-desktop-diagnostics",
      "allow-desktop-theme",
      "allow-desktop-set-theme",
      "allow-desktop-authenticate",
    ]);
    expect(JSON.stringify(capability)).not.toMatch(/(?:fs|shell|opener):/u);
    expect(desktopManifest).not.toMatch(/tauri-plugin-(?:fs|shell|opener)/u);
  });

  test("keeps production and development CSPs limited to the app and local development server", () => {
    expect(desktopConfig.app.security.csp).toEqual(productionCsp);
    expect(desktopConfig.app.security.devCsp).toEqual({
      ...productionCsp,
      "connect-src": "ipc: http://ipc.localhost http://localhost:1420 ws://localhost:1420",
    });
    expect(JSON.stringify(desktopConfig.app.security.csp)).not.toMatch(/unsafe-(?:eval|inline)/u);
  });

  test("sets browser isolation headers and creates the main webview through the security policy", () => {
    expect(desktopConfig.app.security.freezePrototype).toBe(true);
    expect(desktopConfig.app.security.headers).toEqual({
      "Cross-Origin-Opener-Policy": "same-origin",
      "Cross-Origin-Resource-Policy": "same-origin",
      "Permissions-Policy": "camera=(), microphone=(), geolocation=(), payment=(), usb=()",
      "X-Content-Type-Options": "nosniff",
    });
    expect(desktopConfig.app.windows).toContainEqual({
      create: false,
      height: 760,
      minHeight: 600,
      minWidth: 900,
      title: "Cipher",
      width: 1100,
    });
    expect(desktopEntryPoint).toContain(".on_navigation(security::allows_navigation)");
    expect(desktopEntryPoint).toContain("tauri::webview::NewWindowResponse::Deny");
    expect(desktopEntryPoint).toContain(".on_download(|_, _| false)");
    expect(desktopEntryPoint).toContain("tauri_plugin_single_instance::init");
    expect(desktopEntryPoint).toContain("lifecycle::handle_single_instance_launch");
  });
});

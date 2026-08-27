import { defineConfig, lazyPlugins } from "vite-plus";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

export default defineConfig({
  plugins: lazyPlugins(() => [tailwindcss(), react()]) ?? [],
  clearScreen: false,
  lint: {
    ignorePatterns: [
      ".codeql/**",
      "**/target/**",
      "coverage/**",
      "dist/**",
      "infra/**",
      "infra/cdk.out/**",
      "infra/node_modules/**",
      "node_modules/**",
      "src-tauri/gen/**",
    ],
    rules: {
      complexity: "off",
    },
    overrides: [
      {
        files: ["src/**/*.{js,jsx,ts,tsx}"],
        rules: {
          complexity: ["error", { max: 15, variant: "classic" }],
        },
      },
      {
        files: ["test/**/*.test.{ts,tsx}"],
        rules: {
          "typescript/await-thenable": "off",
        },
      },
    ],
    options: {
      typeAware: true,
      typeCheck: true,
    },
  },
  fmt: {
    endOfLine: "lf",
    ignorePatterns: [
      ".codeql/**",
      "bun.lock",
      "**/Cargo.toml",
      "coverage/**",
      "dist/**",
      "infra/cdk.out/**",
      "infra/node_modules/**",
      "node_modules/**",
      "src-tauri/gen/**",
      "src-tauri/target/**",
      "target/**",
    ],
    printWidth: 100,
    proseWrap: "preserve",
    semi: true,
    singleQuote: false,
    sortPackageJson: false,
    tabWidth: 2,
    trailingComma: "all",
  },
  server: {
    port: 1420,
    strictPort: true,
  },
});

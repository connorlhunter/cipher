import { RouterProvider } from "@tanstack/react-router";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { type JSX, useEffect, useState } from "react";

import { router } from "./routes";
import { listenForRendererPurgeEvents } from "./desktop";
import {
  createBrowserRendererDataLifetime,
  type RendererDataLifetime,
} from "./renderer-data-lifetime";
import { ThemeProvider } from "./features/theme/theme-provider";

/**
 * @returns The desktop shell and its native lifecycle cleanup boundary.
 */
export function App(): JSX.Element {
  const [rendererData] = useState<RendererDataLifetime>(() => createBrowserRendererDataLifetime());
  const [queryClient] = useState(
    () =>
      new QueryClient({
        defaultOptions: {
          mutations: { retry: false },
          queries: { retry: false, staleTime: 0 },
        },
      }),
  );

  useEffect(() => {
    let disposed = false;
    let stopListening: (() => Promise<void>) | undefined;

    void (async (): Promise<void> => {
      await rendererData.clear();
      if (disposed) {
        return;
      }

      try {
        stopListening = await listenForRendererPurgeEvents((reason) => rendererData.purge(reason));
      } catch {
        // Older desktop cores do not emit renderer cleanup lifecycle events yet.
      }
    })();

    return () => {
      disposed = true;
      if (stopListening !== undefined) {
        void stopListening();
      }
      void rendererData.clear();
    };
  }, [rendererData]);

  return (
    <ThemeProvider>
      <QueryClientProvider client={queryClient}>
        <RouterProvider router={router} />
      </QueryClientProvider>
    </ThemeProvider>
  );
}

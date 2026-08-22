import { type JSX, useEffect, useState } from "react";

import { desktopStatus, listenForRendererPurgeEvents, type DesktopStatus } from "./desktop";
import {
  createBrowserRendererDataLifetime,
  type RendererDataLifetime,
} from "./renderer-data-lifetime";

/**
 * @returns The desktop shell and its current native-core status.
 */
export function App(): JSX.Element {
  const [status, setStatus] = useState<DesktopStatus | null>(null);
  const [failed, setFailed] = useState(false);
  const [rendererData] = useState<RendererDataLifetime>(() => createBrowserRendererDataLifetime());

  useEffect(() => {
    void desktopStatus()
      .then(setStatus)
      .catch(() => setFailed(true));
  }, []);

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
    <main>
      <p className="eyebrow">Cipher</p>
      <h1>Desktop shell</h1>
      <p>Native login and encrypted messaging will be added here.</p>
      <p className="status">
        {failed
          ? "Unable to reach the desktop core."
          : (status?.message ?? "Starting desktop core...")}
      </p>
    </main>
  );
}

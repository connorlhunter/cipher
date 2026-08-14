import { type JSX, useEffect, useState } from "react";

import { desktopStatus, type DesktopStatus } from "./desktop";

export function App(): JSX.Element {
  const [status, setStatus] = useState<DesktopStatus | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    void desktopStatus()
      .then(setStatus)
      .catch(() => setFailed(true));
  }, []);

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

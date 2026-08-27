import {
  createContext,
  type JSX,
  type ReactNode,
  useContext,
  useEffect,
  useLayoutEffect,
  useMemo,
  useState,
} from "react";

import {
  desktopTheme,
  listenForDesktopThemeChanges,
  setDesktopTheme,
  type DesktopTheme,
  type DesktopThemePreference,
} from "../../desktop";

export interface NativeThemeBoundary {
  current(): Promise<DesktopTheme>;
  set(preference: DesktopThemePreference): Promise<DesktopTheme>;
  subscribe(refresh: () => Promise<void>): Promise<() => void | Promise<void>>;
}

const desktopThemeBoundary: NativeThemeBoundary = {
  current: desktopTheme,
  set: setDesktopTheme,
  subscribe: listenForDesktopThemeChanges,
};

interface ThemeContextValue {
  unavailable: boolean;
  pending: boolean;
  preference: DesktopThemePreference;
  resolved: DesktopTheme["resolved"];
  scheme: DesktopTheme["scheme"];
  select: (preference: DesktopThemePreference) => Promise<void>;
}

const fallbackTheme: DesktopTheme = Object.freeze({
  preference: "system",
  scheme: "atlas",
  resolved: "light",
});

const ThemeContext = createContext<ThemeContextValue | null>(null);

export interface ThemeProviderProps {
  boundary?: NativeThemeBoundary;
  children: ReactNode;
}

/** Applies the native-resolved theme to the document without browser preference storage. */
export function applyDesktopTheme(documentElement: HTMLElement, theme: DesktopTheme): void {
  documentElement.dataset.scheme = theme.scheme;
  documentElement.dataset.theme = theme.resolved;
  documentElement.dataset.themePreference = theme.preference;
  documentElement.style.colorScheme = theme.resolved;
}

/** Provides one native-owned system or explicit color-scheme preference to every app surface. */
export function ThemeProvider({
  boundary = desktopThemeBoundary,
  children,
}: ThemeProviderProps): JSX.Element {
  const [theme, setTheme] = useState<DesktopTheme>(fallbackTheme);
  const [pending, setPending] = useState(false);
  const [unavailable, setUnavailable] = useState(false);

  useLayoutEffect(() => {
    applyDesktopTheme(document.documentElement, theme);
  }, [theme]);

  useEffect(() => {
    let disposed = false;
    let stop: (() => void | Promise<void>) | undefined;

    const refresh = async (): Promise<void> => {
      try {
        const next = await boundary.current();
        if (!disposed) {
          setTheme(next);
          setUnavailable(false);
        }
      } catch {
        // A pre-theme native core leaves the safe initial system-light fallback in place.
      }
    };

    void (async (): Promise<void> => {
      await refresh();
      if (disposed) {
        return;
      }

      try {
        stop = await boundary.subscribe(refresh);
      } catch {
        // The current theme remains usable when an older desktop core has no notification.
      }
    })();

    return () => {
      disposed = true;
      if (stop !== undefined) {
        void stop();
      }
    };
  }, [boundary]);

  const value = useMemo<ThemeContextValue>(
    () => ({
      pending,
      unavailable,
      preference: theme.preference,
      resolved: theme.resolved,
      scheme: theme.scheme,
      select: async (preference: DesktopThemePreference): Promise<void> => {
        setPending(true);
        try {
          setTheme(await boundary.set(preference));
          setUnavailable(false);
        } catch {
          setUnavailable(true);
        } finally {
          setPending(false);
        }
      },
    }),
    [boundary, pending, theme, unavailable],
  );

  return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>;
}

/** Returns the safe application-wide theme state. */
export function useTheme(): ThemeContextValue {
  const value = useContext(ThemeContext);
  if (value === null) {
    throw new Error("Cipher theme controls must be inside the theme boundary.");
  }
  return value;
}

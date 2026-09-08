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

  // The cleanup awaits the subscription promise, including setup that finishes after unmount.
  // react-doctor-disable-next-line react-doctor/effect-needs-cleanup
  useEffect(() => {
    let disposed = false;

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

    const subscription = refresh()
      .then(() => (disposed ? undefined : boundary.subscribe(refresh)))
      .catch(() => undefined);

    return () => {
      disposed = true;
      // Subscription setup may finish after unmount; its disposer still belongs to this effect.
      void subscription.then((stop) => stop?.()).catch(() => undefined);
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

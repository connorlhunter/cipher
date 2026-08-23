import { type JSX, type ReactNode, useEffect, useRef } from "react";
import { useRouterState } from "@tanstack/react-router";

/** Moves focus to new route content without exposing focus history as application state. */
export function focusRouteContent(element: HTMLElement | null): void {
  element?.focus({ preventScroll: true });
}

/** Holds a route's focusable reading region. */
export function RouteFocusFrame({
  children,
  pathname,
}: {
  children: ReactNode;
  pathname: string;
}): JSX.Element {
  const main = useRef<HTMLElement>(null);
  const previousPathname = useRef(pathname);

  useEffect(() => {
    if (previousPathname.current === pathname) {
      return;
    }
    previousPathname.current = pathname;
    focusRouteContent(main.current);
  }, [pathname]);

  return (
    <main
      className="min-w-0 overflow-auto p-5 sm:p-7 lg:p-9"
      id="main-content"
      ref={main}
      tabIndex={-1}
    >
      {children}
    </main>
  );
}

/** Restores a predictable reading position after an in-app route transition. */
export function RouteFocusRestoration({ children }: { children: ReactNode }): JSX.Element {
  const pathname = useRouterState({ select: (state) => state.location.pathname });

  return <RouteFocusFrame pathname={pathname}>{children}</RouteFocusFrame>;
}

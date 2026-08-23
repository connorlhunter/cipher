import { type JSX, useEffect, useRef } from "react";
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
} from "@tanstack/react-router";

import { DesktopShell } from "../app/desktop-shell";
import { Badge } from "../components/ui/badge";
import { Card } from "../components/ui/card";
import { ThemePreferenceControl } from "../features/theme/theme-preference-control";
import { SignInForm } from "../features/auth/sign-in-form";

/** A safe fallback for route-level failures that never serializes the original error. */
export function RouteErrorFallback(): JSX.Element {
  const fallback = useRef<HTMLElement>(null);

  useEffect(() => {
    fallback.current?.focus({ preventScroll: true });
  }, []);

  return (
    <main
      className="grid min-h-dvh place-items-center bg-canvas p-panel text-text"
      ref={fallback}
      tabIndex={-1}
    >
      <section aria-labelledby="route-error-title" className="w-full max-w-2xl py-8">
        <Badge tone="destructive">Unavailable</Badge>
        <h1 className="type-page-title mt-paragraph" id="route-error-title">
          This view is unavailable
        </h1>
        <p className="type-body-muted mt-paragraph max-w-prose">
          Return to the overview or restart Cipher and try again.
        </p>
      </section>
    </main>
  );
}

/** The desktop starting point before account-specific features are available. */
export function OverviewRoute(): JSX.Element {
  return (
    <section
      aria-labelledby="overview-title"
      className="mx-auto grid max-w-3xl gap-section py-5 lg:py-9"
    >
      <div>
        <Badge tone="success">Desktop ready</Badge>
        <h1 className="type-display mt-paragraph" id="overview-title">
          A calm, focused place for Cipher
        </h1>
        <p className="type-body-muted mt-paragraph max-w-prose">
          The desktop shell is ready for secure conversation features. Appearance, focus, and layout
          behavior are shared across every view.
        </p>
      </div>
      <dl className="grid gap-px overflow-hidden rounded-lg border border-border bg-border sm:grid-cols-3">
        <Metadata label="Appearance" value="Native-controlled" />
        <Metadata label="Typography" value="System UI" />
        <Metadata label="Layout" value="Zoom-ready" />
      </dl>
    </section>
  );
}

/** An initial route proving the shared shell can host settings without a popout. */
export function AppearanceRoute(): JSX.Element {
  return (
    <section aria-labelledby="appearance-title" className="mx-auto max-w-3xl py-5 lg:py-9">
      <Badge tone="neutral">Settings</Badge>
      <h1 className="type-page-title mt-paragraph" id="appearance-title">
        Appearance
      </h1>
      <p className="type-body-muted mt-paragraph max-w-prose">
        Choose system appearance or an explicit color scheme. Cipher stores that non-secret
        preference in native application configuration and applies one resolved appearance to the
        desktop frame and webview.
      </p>
      <Card className="mt-section p-panel">
        <h2 className="type-section-title">Theme</h2>
        <p className="type-body-muted mt-paragraph">
          System follows the current operating-system appearance; explicit schemes keep their own
          light or dark native window treatment.
        </p>
        <div className="mt-panel">
          <ThemePreferenceControl />
        </div>
      </Card>
    </section>
  );
}

/** The credential-entry route keeps browser history and persisted state free of secrets. */
export function SignInRoute(): JSX.Element {
  return (
    <section className="mx-auto grid max-w-3xl place-items-start py-5 lg:py-9">
      <SignInForm />
    </section>
  );
}

function Metadata({ label, value }: { label: string; value: string }): JSX.Element {
  return (
    <div className="bg-surface px-4 py-4">
      <dt className="type-label uppercase tracking-[0.12em] text-muted">{label}</dt>
      <dd className="type-caption mt-2 font-medium text-text">{value}</dd>
    </div>
  );
}

const rootRoute = createRootRoute({
  component: DesktopShell,
  errorComponent: RouteErrorFallback,
});

const overviewRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/",
  component: OverviewRoute,
});

const appearanceRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/settings/appearance",
  component: AppearanceRoute,
});

const signInRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/sign-in",
  component: SignInRoute,
});

const routeTree = rootRoute.addChildren([overviewRoute, appearanceRoute, signInRoute]);

/** The in-memory router avoids putting sensitive state into location history. */
export const router = createRouter({
  history: createMemoryHistory({ initialEntries: ["/"] }),
  routeTree,
});

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}

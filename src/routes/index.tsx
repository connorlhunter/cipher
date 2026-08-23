import { type JSX, useEffect, useRef } from "react";
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Link,
} from "@tanstack/react-router";
import { LockKeyhole, LogIn, UserPlus } from "lucide-react";

import { DesktopShell } from "../app/desktop-shell";
import { Badge } from "../components/ui/badge";
import { buttonVariants } from "../components/ui/button";
import { Card } from "../components/ui/card";
import { DeviceSettings } from "../features/settings/device-settings";
import { ThemePreferenceControl } from "../features/theme/theme-preference-control";
import { PasswordResetForm, SignInForm } from "../features/auth/sign-in-form";
import { cn } from "../lib/utils";

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
      className="mx-auto grid min-h-full max-w-2xl place-items-center py-5 text-center lg:py-9"
    >
      <div className="grid justify-items-center">
        <div className="cipher-welcome-mark flex size-20 items-center justify-center rounded-[1.375rem] bg-elevated p-3">
          <img alt="Cipher" className="size-full" src="/cipher-mark.svg" />
        </div>
        <div className="cipher-welcome-content grid justify-items-center">
          <h1 className="type-display mt-section" id="overview-title">
            Cipher
          </h1>
          <p className="type-body-muted mt-paragraph max-w-prose">
            A private space for the conversations that matter most.
          </p>
          <p
            aria-label="End-to-end encryption"
            className="type-caption mt-paragraph inline-flex items-center gap-2 text-muted"
            title="End-to-end encrypted: only you and the people you message can read your conversations."
          >
            <LockKeyhole aria-hidden="true" size={15} strokeWidth={1.8} />
            Built for E2EE
          </p>
          <div className="mt-section flex flex-wrap justify-center gap-3">
            <Link className={buttonVariants()} to="/sign-in">
              <LogIn aria-hidden="true" size={17} strokeWidth={1.8} />
              Log in
            </Link>
            <button
              aria-describedby="sign-up-coming-soon"
              className={cn(buttonVariants({ variant: "secondary" }))}
              disabled
              type="button"
            >
              <UserPlus aria-hidden="true" size={17} strokeWidth={1.8} />
              Sign up
            </button>
          </div>
          <p className="type-caption mt-2 text-muted" id="sign-up-coming-soon">
            Sign up is coming soon.
          </p>
        </div>
      </div>
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
        Choose the look that feels right for you.
      </p>
      <Card className="mt-section p-panel">
        <h2 className="type-section-title">Theme</h2>
        <p className="type-body-muted mt-paragraph">Use your system setting or choose a theme.</p>
        <div className="mt-panel">
          <ThemePreferenceControl />
        </div>
      </Card>
      <DeviceSettings />
    </section>
  );
}

/** The credential-entry route keeps browser history and persisted state free of secrets. */
export function SignInRoute(): JSX.Element {
  return (
    <section className="mx-auto grid min-h-full max-w-3xl place-items-center py-5 lg:py-9">
      <SignInForm />
    </section>
  );
}

/** Account recovery is a dedicated route instead of an in-form detour. */
export function PasswordResetRoute(): JSX.Element {
  return (
    <section className="mx-auto grid min-h-full max-w-3xl place-items-center py-5 lg:py-9">
      <PasswordResetForm />
    </section>
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

const passwordResetRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/password-reset",
  component: PasswordResetRoute,
});

const routeTree = rootRoute.addChildren([
  overviewRoute,
  appearanceRoute,
  signInRoute,
  passwordResetRoute,
]);

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

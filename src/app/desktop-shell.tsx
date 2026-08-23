import {
  CircleHelp,
  KeyRound,
  LayoutPanelTop,
  MessageSquare,
  Settings2,
  Sparkles,
} from "lucide-react";
import { type JSX, type ReactNode } from "react";
import { Link, Outlet } from "@tanstack/react-router";

import { Badge } from "../components/ui/badge";
import { Separator } from "../components/ui/separator";
import { ThemePreferenceControl } from "../features/theme/theme-preference-control";
import { cn } from "../lib/utils";
import { focusRouteContent, RouteFocusRestoration } from "./focus-restoration";

const navigation = [
  { icon: LayoutPanelTop, label: "Overview", to: "/" },
  { icon: KeyRound, label: "Sign in", to: "/sign-in" },
  { icon: Settings2, label: "Appearance", to: "/settings/appearance" },
] as const;

/** The theme-aware title bar, header, primary navigation, and responsive desktop grid. */
export function DesktopShell(): JSX.Element {
  return (
    <div className="app-shell min-h-dvh bg-canvas text-text">
      <a
        className="skip-link"
        href="#main-content"
        onClick={(event) => {
          event.preventDefault();
          focusRouteContent(document.getElementById("main-content"));
        }}
      >
        Skip to content
      </a>
      <header className="app-header border-border bg-surface text-text">
        <div className="flex min-w-0 items-center gap-3" data-tauri-drag-region>
          <div className="flex size-8 shrink-0 items-center justify-center rounded-md bg-accent text-on-accent">
            <Sparkles aria-hidden="true" size={17} strokeWidth={1.8} />
          </div>
          <div className="min-w-0">
            <p className="type-label truncate">Cipher</p>
            <p className="type-caption truncate text-muted">Desktop</p>
          </div>
        </div>
        <div className="ml-auto flex shrink-0 items-center gap-2">
          <ThemePreferenceControl />
        </div>
      </header>
      <nav aria-label="Primary" className="primary-rail border-border bg-surface">
        <div className="flex min-w-0 flex-col gap-1">
          <p className="type-label px-2 pb-1 pt-1 uppercase tracking-[0.12em] text-muted">
            Workspace
          </p>
          {navigation.map(({ icon: Icon, label, to }) => (
            <Link
              activeProps={{
                "aria-current": "page",
                className: "bg-elevated text-text shadow-xs",
              }}
              className={cn(
                "type-label flex min-h-control items-center gap-3 rounded-md px-3 text-muted transition-colors hover:bg-elevated hover:text-text motion-reduce:transition-none",
              )}
              key={to}
              to={to}
            >
              <Icon aria-hidden="true" size={18} strokeWidth={1.8} />
              <span>{label}</span>
            </Link>
          ))}
        </div>
        <div className="mt-auto pt-5">
          <Separator />
          <div className="type-caption mt-3 flex min-h-control items-center gap-3 rounded-md px-3 text-muted">
            <CircleHelp aria-hidden="true" size={18} strokeWidth={1.8} />
            Security-first desktop
          </div>
        </div>
      </nav>
      <RouteFocusRestoration>
        <Outlet />
      </RouteFocusRestoration>
      <SecondaryRail>
        <div className="flex items-start gap-3">
          <div className="flex size-9 shrink-0 items-center justify-center rounded-md bg-elevated text-accent">
            <MessageSquare aria-hidden="true" size={18} strokeWidth={1.8} />
          </div>
          <div className="min-w-0">
            <h2 className="type-section-title">Workspace context</h2>
            <p className="type-body-muted mt-paragraph">
              Contextual details will appear here when messaging is available.
            </p>
          </div>
        </div>
        <Separator className="my-5" />
        <div className="flex items-center justify-between gap-3">
          <span className="type-caption text-muted">Native connection</span>
          <Badge tone="neutral">Starting</Badge>
        </div>
      </SecondaryRail>
    </div>
  );
}

/** A narrow, optional context column that leaves the reading surface usable at high zoom. */
export function SecondaryRail({ children }: { children: ReactNode }): JSX.Element {
  return (
    <aside aria-label="Workspace context" className="secondary-rail border-border bg-surface p-5">
      {children}
    </aside>
  );
}

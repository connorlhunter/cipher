import { type HTMLAttributes, type JSX } from "react";

import { cn } from "../../lib/utils";

/** A reviewed surface primitive for dense desktop content and future dialogs. */
export function Card({ className, ...props }: HTMLAttributes<HTMLElement>): JSX.Element {
  return (
    <section
      className={cn("rounded-lg border border-border bg-surface shadow-sm", className)}
      {...props}
    />
  );
}

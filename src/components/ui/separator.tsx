import { type HTMLAttributes, type JSX } from "react";

import { cn } from "../../lib/utils";

export interface SeparatorProps extends HTMLAttributes<HTMLDivElement> {
  orientation?: "horizontal" | "vertical";
}

/** A decorative boundary that inherits the shared shell border token. */
export function Separator({
  className,
  orientation = "horizontal",
  ...props
}: SeparatorProps): JSX.Element {
  return (
    <div
      aria-orientation={orientation}
      className={cn(
        "shrink-0 bg-border",
        orientation === "horizontal" ? "h-px w-full" : "h-full w-px",
        className,
      )}
      role="separator"
      {...props}
    />
  );
}

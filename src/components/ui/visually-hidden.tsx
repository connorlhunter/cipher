import { type HTMLAttributes, type JSX } from "react";

import { cn } from "../../lib/utils";

/** Makes supporting text available to assistive technology without visual noise. */
export function VisuallyHidden({
  className,
  ...props
}: HTMLAttributes<HTMLSpanElement>): JSX.Element {
  return <span className={cn("sr-only", className)} {...props} />;
}

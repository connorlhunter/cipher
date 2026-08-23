import { type LabelHTMLAttributes, type JSX } from "react";

import { cn } from "../../lib/utils";

/** A compact label primitive that keeps form instructions associated with inputs. */
export function Label({ className, ...props }: LabelHTMLAttributes<HTMLLabelElement>): JSX.Element {
  return <label className={cn("type-label text-text", className)} {...props} />;
}

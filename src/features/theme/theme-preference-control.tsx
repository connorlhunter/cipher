import { Monitor, Moon, Sun } from "lucide-react";
import { type JSX } from "react";

import { Button } from "../../components/ui/button";
import { cn } from "../../lib/utils";
import { type DesktopThemePreference } from "../../desktop";
import { useTheme } from "./theme-provider";

const choices: ReadonlyArray<{
  icon: typeof Monitor;
  label: string;
  value: DesktopThemePreference;
}> = [
  { icon: Monitor, label: "System", value: "system" },
  { icon: Sun, label: "Light", value: "light" },
  { icon: Moon, label: "Dark", value: "dark" },
];

/** Chooses the single native-owned appearance preference without WebView persistence. */
export function ThemePreferenceControl(): JSX.Element {
  const { pending, preference, resolved, select, unavailable } = useTheme();

  return (
    <fieldset className="flex min-w-0 items-center gap-1" disabled={pending}>
      <legend className="sr-only">Appearance</legend>
      {choices.map(({ icon: Icon, label, value }) => {
        const selected = preference === value;
        return (
          <Button
            aria-label={`${label} appearance`}
            aria-pressed={selected}
            className={cn(
              "shrink-0",
              selected && "border border-border bg-elevated text-accent shadow-xs",
            )}
            key={value}
            onClick={() => void select(value)}
            size="compact"
            title={`${label} appearance`}
            variant="ghost"
          >
            <Icon aria-hidden="true" size={16} strokeWidth={1.8} />
            <span className="hidden xl:inline">{label}</span>
          </Button>
        );
      })}
      <output aria-live="polite" className="sr-only">
        {pending
          ? "Updating appearance"
          : unavailable
            ? "Appearance controls are unavailable"
            : `${resolved} appearance active`}
      </output>
    </fieldset>
  );
}

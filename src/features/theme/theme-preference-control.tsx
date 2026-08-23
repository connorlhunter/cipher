import { Monitor, Palette } from "lucide-react";
import { type JSX, useId } from "react";

import { Button } from "../../components/ui/button";
import { Select } from "../../components/ui/select";
import { desktopThemePreferences, type DesktopThemePreference } from "../../desktop-contract";
import { useTheme } from "./theme-provider";

const labels = Object.freeze({
  system: "System",
  atlas: "Atlas",
  paper: "Paper",
  citrine: "Citrine",
  harbor: "Harbor",
  midnight: "Midnight",
  onyx: "Onyx",
  rose: "Rose",
  tide: "Tide",
  ember: "Ember",
  quartz: "Quartz",
} satisfies Record<DesktopThemePreference, string>);

/** Returns the next native preference in the accessible appearance cycle. */
export function nextThemePreference(preference: DesktopThemePreference): DesktopThemePreference {
  const index = desktopThemePreferences.indexOf(preference);
  return desktopThemePreferences[(index + 1) % desktopThemePreferences.length] ?? "system";
}

/** Selects or cycles the complete native-owned appearance set without WebView persistence. */
export function ThemePreferenceControl(): JSX.Element {
  const selectId = useId();
  const { pending, preference, resolved, scheme, select, unavailable } = useTheme();
  const next = nextThemePreference(preference);

  return (
    <fieldset className="flex min-w-0 items-center gap-1" disabled={pending}>
      <legend className="sr-only">Appearance</legend>
      <label className="sr-only" htmlFor={selectId}>
        Color scheme
      </label>
      <Select
        aria-label="Color scheme"
        className="max-w-28 sm:max-w-36"
        id={selectId}
        onChange={(event) => void select(event.currentTarget.value as DesktopThemePreference)}
        value={preference}
      >
        {desktopThemePreferences.map((value) => (
          <option key={value} value={value}>
            {value === "system" ? "System (automatic)" : labels[value]}
          </option>
        ))}
      </Select>
      <Button
        aria-label={`Use ${labels[next]} appearance`}
        onClick={() => void select(next)}
        size="compact"
        title={`Use ${labels[next]} appearance`}
        variant="ghost"
      >
        {next === "system" ? (
          <Monitor aria-hidden="true" size={16} strokeWidth={1.8} />
        ) : (
          <Palette aria-hidden="true" size={16} strokeWidth={1.8} />
        )}
      </Button>
      <output aria-live="polite" className="sr-only">
        {pending
          ? "Updating appearance"
          : unavailable
            ? "Appearance controls are unavailable"
            : `${labels[preference]} preference active with the ${labels[scheme]} ${resolved} scheme`}
      </output>
    </fieldset>
  );
}

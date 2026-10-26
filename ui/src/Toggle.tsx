import type { ReactNode } from "react";

/**
 * An on/off setting drawn as a slider switch. Native checkboxes render as
 * round checkmark bubbles in WebKit on iOS and macOS, so on/off settings use
 * this instead. The label wraps the button, so a tap on the text toggles it
 * too; Space and Enter work because it is a real button.
 */
export function Toggle({
  checked,
  onChange,
  disabled = false,
  children,
}: {
  checked: boolean;
  onChange: (next: boolean) => void;
  disabled?: boolean;
  children: ReactNode;
}) {
  return (
    <label className="toggle-row">
      <span className="toggle-text">{children}</span>
      <button
        type="button"
        role="switch"
        aria-checked={checked}
        className="toggle"
        disabled={disabled}
        onClick={() => onChange(!checked)}
      >
        <span className="toggle-thumb" aria-hidden="true" />
      </button>
    </label>
  );
}

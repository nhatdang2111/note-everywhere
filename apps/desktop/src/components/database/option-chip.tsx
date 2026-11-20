import type { JSX } from "solid-js";
import type { OptionColor, SelectOption } from "../../api/contract.ts";

/** A coloured tag for a select option; the colour pairs are tokens, so they hold their contrast in both themes. */
export function OptionChip(props: { name: string; color: OptionColor; onRemove?: () => void; removeLabel?: string }): JSX.Element {
  return (
    <span class="opt" data-color={props.color}>
      <span class="opt-name">{props.name}</span>
      {props.onRemove && (
        <button type="button" class="opt-remove" aria-label={props.removeLabel} tabindex="-1" onClick={props.onRemove}>
          ×
        </button>
      )}
    </span>
  );
}

export const chipOf = (option: SelectOption): JSX.Element => <OptionChip name={option.name} color={option.color} />;

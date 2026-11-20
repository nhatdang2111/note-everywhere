import type { JSX } from "solid-js";
import { t, type MessageKey } from "../../i18n/index.ts";
import { MIN_PASSWORD_LENGTH, passwordStrength } from "../../lib/password.ts";

const LABELS: Record<ReturnType<typeof passwordStrength>["level"], MessageKey> = {
  tooShort: "strength.tooShort",
  weak: "strength.weak",
  fair: "strength.fair",
  good: "strength.good",
  strong: "strength.strong",
};

/** Four-segment meter with a text label, so colour is never the only signal. */
export function PasswordStrengthMeter(props: { password: string }): JSX.Element {
  const strength = () => passwordStrength(props.password);
  return (
    <div class="strength" data-score={strength().score} aria-live="polite">
      <div class="strength-bars" aria-hidden="true">
        {[1, 2, 3, 4].map((step) => (
          <span class="strength-bar" data-on={strength().score >= step ? "true" : "false"} />
        ))}
      </div>
      <span class="strength-label">
        {props.password.length === 0 ? t("strength.hint", { min: MIN_PASSWORD_LENGTH }) : t(LABELS[strength().level], { min: MIN_PASSWORD_LENGTH })}
      </span>
    </div>
  );
}

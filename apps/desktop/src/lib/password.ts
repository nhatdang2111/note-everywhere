/** Shortest password the UI accepts for an account or a vault; the core may demand more. */
export const MIN_PASSWORD_LENGTH = 10;

export type StrengthLevel = "tooShort" | "weak" | "fair" | "good" | "strong";

export interface Strength {
  level: StrengthLevel;
  /** 0 (too short) to 4 (strong), for a meter. */
  score: 0 | 1 | 2 | 3 | 4;
  bits: number;
}

/**
 * A rough guide, not a guarantee: character-pool entropy, discounted for
 * repeated characters and runs like "abcd" or "1234". It exists to nudge people
 * towards longer passwords; the core makes the real decision.
 */
export function passwordStrength(password: string): Strength {
  if (password.length < MIN_PASSWORD_LENGTH) return { level: "tooShort", score: 0, bits: 0 };
  let pool = 0;
  if (/[a-z]/.test(password)) pool += 26;
  if (/[A-Z]/.test(password)) pool += 26;
  if (/\d/.test(password)) pool += 10;
  if (/[^A-Za-z0-9]/.test(password)) pool += 33;

  const unique = new Set(password).size;
  let runs = 0;
  for (let i = 2; i < password.length; i += 1) {
    const a = password.charCodeAt(i - 2);
    const b = password.charCodeAt(i - 1);
    const c = password.charCodeAt(i);
    if (b - a === c - b && Math.abs(b - a) <= 1) runs += 1;
  }
  const effectiveLength = Math.max(1, Math.min(password.length, unique * 2) - runs);
  const bits = Math.round(effectiveLength * Math.log2(Math.max(pool, 2)));

  if (bits < 40) return { level: "weak", score: 1, bits };
  if (bits < 60) return { level: "fair", score: 2, bits };
  if (bits < 80) return { level: "good", score: 3, bits };
  return { level: "strong", score: 4, bits };
}

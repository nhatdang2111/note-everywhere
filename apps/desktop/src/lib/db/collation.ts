/**
 * Vietnamese collation for sorting text: letters follow the Vietnamese
 * alphabet (a, ă, â, b, c, d, đ, e, ê, ...) and tone marks only break ties
 * between otherwise equal letters (none, grave, hook, tilde, acute, dot).
 * Digits and punctuation sort before letters by code point.
 */
const ALPHABET = "aăâbcdđeêfghijklmnoôơpqrstuưvwxyz";
const LETTER_MODIFIERS = new Set(["̆", "̂", "̛"]);
const TONES = ["̀", "̉", "̃", "́", "̣"];
const LETTER_BASE = 0x200000;

interface CollationKey {
  primary: number[];
  tones: number[];
}

function keyOf(text: string): CollationKey {
  const primary: number[] = [];
  const tones: number[] = [];
  for (const char of text.normalize("NFC").toLowerCase()) {
    const parts = char.normalize("NFD");
    const base = parts[0] ?? char;
    const marks = [...parts.slice(1)];
    const modifier = marks.find((mark) => LETTER_MODIFIERS.has(mark)) ?? "";
    const tone = marks.find((mark) => TONES.includes(mark));
    const rank = ALPHABET.indexOf((base + modifier).normalize("NFC"));
    primary.push(rank >= 0 ? LETTER_BASE + rank : (char.codePointAt(0) ?? 0));
    tones.push(tone ? TONES.indexOf(tone) + 1 : 0);
  }
  return { primary, tones };
}

function compareNumbers(a: number[], b: number[]): number {
  const length = Math.min(a.length, b.length);
  for (let at = 0; at < length; at += 1) {
    const diff = (a[at] as number) - (b[at] as number);
    if (diff !== 0) return diff;
  }
  return a.length - b.length;
}

export function vietnameseCompare(a: string, b: string): number {
  const left = keyOf(a);
  const right = keyOf(b);
  return compareNumbers(left.primary, right.primary) || compareNumbers(left.tones, right.tones);
}

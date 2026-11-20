/**
 * Fold one UTF-16 unit: strip diacritics and case so "ghi chu" matches
 * "Ghi chú". The mapping is one unit to one unit, so offsets in the folded
 * string are valid offsets in the original.
 */
function foldUnit(ch: string): string {
  if (ch === "đ" || ch === "Đ") return "d";
  const base = ch.normalize("NFD")[0] ?? ch;
  return base.toLowerCase()[0] ?? base;
}

export function fold(text: string): string {
  return text.replace(/[\s\S]/g, foldUnit);
}

/** True when every whitespace-separated word of `query` occurs in `text`, ignoring case and accents. */
export function matchesQuery(text: string, query: string): boolean {
  const haystack = fold(text);
  return fold(query)
    .split(/\s+/)
    .filter(Boolean)
    .every((word) => haystack.includes(word));
}

/** Offsets of every non-overlapping occurrence of `query` in `text`, ignoring case and accents. */
export function findAll(text: string, query: string): number[] {
  const needle = fold(query);
  if (!needle) return [];
  const haystack = fold(text);
  const hits: number[] = [];
  for (let at = haystack.indexOf(needle); at !== -1; at = haystack.indexOf(needle, at + needle.length)) {
    hits.push(at);
  }
  return hits;
}

export function truncate(text: string, max: number): string {
  return text.length <= max ? text : `${text.slice(0, max - 1)}…`;
}

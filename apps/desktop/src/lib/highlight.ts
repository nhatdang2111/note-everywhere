export interface TextRun {
  text: string;
  mark: boolean;
}

/**
 * Cut `text` into runs so that the given `[start, end)` ranges are marked.
 * Ranges may overlap, touch or run past the end; they are merged and clamped.
 */
export function splitHighlights(text: string, ranges: readonly [number, number][]): TextRun[] {
  const merged: [number, number][] = [];
  const clamped = ranges
    .map(([start, end]) => [Math.max(0, start), Math.min(text.length, end)] as [number, number])
    .filter(([start, end]) => end > start)
    .sort((a, b) => a[0] - b[0]);
  for (const range of clamped) {
    const last = merged[merged.length - 1];
    if (last && range[0] <= last[1]) last[1] = Math.max(last[1], range[1]);
    else merged.push([...range]);
  }
  const runs: TextRun[] = [];
  let cursor = 0;
  for (const [start, end] of merged) {
    if (start > cursor) runs.push({ text: text.slice(cursor, start), mark: false });
    runs.push({ text: text.slice(start, end), mark: true });
    cursor = end;
  }
  if (cursor < text.length) runs.push({ text: text.slice(cursor), mark: false });
  return runs;
}

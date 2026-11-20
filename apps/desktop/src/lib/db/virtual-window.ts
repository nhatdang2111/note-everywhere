/** Windowing for long lists: which rows to draw for a scroll position, and which pages of the query to have. */

export interface WindowInput {
  scrollTop: number;
  viewportHeight: number;
  rowHeight: number;
  total: number;
  /** Rows drawn beyond each edge of the viewport. Default 6. */
  overscan?: number;
}

export interface RowWindow {
  /** First row drawn. */
  start: number;
  /** One past the last row drawn. */
  end: number;
  /** Distance from the top of the list to the first drawn row. */
  offsetTop: number;
  /** Height of the whole list, so the scrollbar is the real size. */
  totalHeight: number;
}

export function computeWindow({ scrollTop, viewportHeight, rowHeight, total, overscan = 6 }: WindowInput): RowWindow {
  const totalHeight = Math.max(0, total) * rowHeight;
  if (total <= 0 || rowHeight <= 0) return { start: 0, end: 0, offsetTop: 0, totalHeight };
  const top = Math.min(Math.max(0, scrollTop), Math.max(0, totalHeight - viewportHeight));
  const first = Math.floor(top / rowHeight);
  const last = Math.ceil((top + Math.max(viewportHeight, 0)) / rowHeight);
  const start = Math.max(0, first - overscan);
  const end = Math.min(total, last + overscan);
  return { start, end, offsetTop: start * rowHeight, totalHeight };
}

/** Indexes of the query pages that cover rows `start..end` (end exclusive). */
export function pagesFor(start: number, end: number, pageSize: number): number[] {
  if (end <= start || pageSize <= 0) return [];
  const pages: number[] = [];
  for (let page = Math.floor(start / pageSize); page <= Math.floor((end - 1) / pageSize); page += 1) pages.push(page);
  return pages;
}

/** The `offset` and `limit` to ask the query API for a page. */
export const pageQuery = (page: number, pageSize: number): { offset: number; limit: number } => ({ offset: page * pageSize, limit: pageSize });

/** The scroll position that brings row `index` fully into view, changing nothing when it already is. */
export function scrollToReveal(index: number, rowHeight: number, scrollTop: number, viewportHeight: number): number {
  const top = index * rowHeight;
  const bottom = top + rowHeight;
  if (top < scrollTop) return top;
  if (bottom > scrollTop + viewportHeight) return bottom - viewportHeight;
  return scrollTop;
}

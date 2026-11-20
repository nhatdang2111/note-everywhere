/** Keyboard movement over the cells of a table. Rows are indexes into the whole result, not only the drawn window. */

export interface CellPos {
  row: number;
  col: number;
}

export interface GridSize {
  rows: number;
  cols: number;
  /** Rows a Page Up or Page Down moves. */
  page: number;
}

const clamp = (value: number, max: number) => Math.min(Math.max(value, 0), Math.max(max - 1, 0));

/** The cell a navigation key lands on, or null when the key does not navigate. */
export function moveCell(pos: CellPos, key: string, size: GridSize, modifier = false): CellPos | null {
  const at = (row: number, col: number): CellPos => ({ row: clamp(row, size.rows), col: clamp(col, size.cols) });
  switch (key) {
    case "ArrowUp":
      return modifier ? at(0, pos.col) : at(pos.row - 1, pos.col);
    case "ArrowDown":
      return modifier ? at(size.rows - 1, pos.col) : at(pos.row + 1, pos.col);
    case "ArrowLeft":
      return modifier ? at(pos.row, 0) : at(pos.row, pos.col - 1);
    case "ArrowRight":
      return modifier ? at(pos.row, size.cols - 1) : at(pos.row, pos.col + 1);
    case "Home":
      return modifier ? at(0, 0) : at(pos.row, 0);
    case "End":
      return modifier ? at(size.rows - 1, size.cols - 1) : at(pos.row, size.cols - 1);
    case "PageUp":
      return at(pos.row - size.page, pos.col);
    case "PageDown":
      return at(pos.row + size.page, pos.col);
    default:
      return null;
  }
}

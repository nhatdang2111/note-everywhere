import type { Core } from "../api/index.ts";
import type { NodeId, QueryResult, QueryRow } from "../api/contract.ts";

const PAGE = 1000;

/**
 * The values of one row. The contract reads rows only through `dbQuery`, so this
 * asks for the rows with the same title (a handful, normally) and picks the
 * right one; if that somehow misses, it pages through the database.
 */
export async function loadRowValues(core: Core, databaseId: NodeId, rowId: NodeId, title: string): Promise<{ row: QueryRow | null; titles: QueryResult["titles"] }> {
  const filter = title === "" ? ({ type: "condition", property: "title", operator: "is_empty" } as const) : ({ type: "condition", property: "title", operator: "is", value: title } as const);
  const first = await core.api.dbQuery({ id: databaseId, spec: { filter, limit: PAGE } });
  const hit = first.rows.find((row) => row.id === rowId);
  if (hit) return { row: hit, titles: first.titles };
  for (let offset = 0; offset < first.total; offset += PAGE) {
    const page = await core.api.dbQuery({ id: databaseId, spec: { filter, offset, limit: PAGE } });
    const found = page.rows.find((row) => row.id === rowId);
    if (found) return { row: found, titles: page.titles };
  }
  return { row: null, titles: {} };
}

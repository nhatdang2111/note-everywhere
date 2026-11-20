import { screen, waitFor } from "@solidjs/testing-library";
import type { CellValue, NodeId } from "../../api/contract.ts";
import type { MemoryCore } from "../../api/memory.ts";
import { renderApp, seedLocal, type RenderOptions, type RenderedApp } from "../../test/app-harness.tsx";

export interface Sample {
  db: NodeId;
  status: string;
  open: string;
  done: string;
  points: string;
  due: string;
  tags: string;
  flag: string;
  link: string;
  rows: NodeId[];
}

/** A database of three tasks with one property of each kind a table cell can hold. */
export async function seedTasks(memory: MemoryCore): Promise<Sample> {
  await seedLocal(memory);
  const { api } = memory;
  const db = await api.dbCreate({ parentId: null, title: "Tasks" });
  const status = await api.dbPropertyAdd({ id: db, name: "Status", type: "select" });
  const open = await api.dbOptionAdd({ id: db, propertyId: status, name: "Open", color: "blue" });
  const done = await api.dbOptionAdd({ id: db, propertyId: status, name: "Done", color: "green" });
  const tags = await api.dbPropertyAdd({ id: db, name: "Tags", type: "multi_select" });
  await api.dbOptionAdd({ id: db, propertyId: tags, name: "Urgent", color: "red" });
  const points = await api.dbPropertyAdd({ id: db, name: "Points", type: "number" });
  const due = await api.dbPropertyAdd({ id: db, name: "Due", type: "date" });
  const flag = await api.dbPropertyAdd({ id: db, name: "Flag", type: "checkbox" });
  const link = await api.dbPropertyAdd({ id: db, name: "Link", type: "url" });
  const make = (title: string, values: Record<string, CellValue> = {}) => api.dbRowCreate({ id: db, title, values });
  const rows = [
    await make("Write report", { [status]: { type: "select", value: open }, [points]: { type: "number", value: 5 } }),
    await make("Ship release", { [status]: { type: "select", value: done }, [due]: { type: "date", value: { start: "2026-11-20" } } }),
    await make("Plan trip"),
  ];
  return { db, status, open, done, points, due, tags, flag, link, rows };
}

export async function renderDatabase(options: RenderOptions & { seed?: (memory: MemoryCore) => Promise<Sample> } = {}) {
  let sample!: Sample;
  const { seed = seedTasks, ...rest } = options;
  const ui = await renderApp({
    ...rest,
    setup: async (memory) => {
      sample = await seed(memory);
      await rest.setup?.(memory);
    },
  });
  await screen.findByRole("navigation", { name: "Workspace" });
  await ui.app.tree.refresh();
  ui.app.openPage(sample.db);
  return { ui, sample };
}

export const cellAt = (row: number, col: number): HTMLElement => {
  const found = document.querySelector<HTMLElement>(`[role="gridcell"][data-row="${row}"][data-col="${col}"]`);
  if (!found) throw new Error(`no cell at ${row},${col}`);
  return found;
};

export const waitForTable = (): Promise<HTMLElement> => waitFor(() => screen.getByRole("grid", { name: "Rows of the database" }));

/** Column index of a property in the default view (the title is column 0). */
export const columnOf = (sample: Sample, id: string): number => [sample.status, sample.tags, sample.points, sample.due, sample.flag, sample.link].indexOf(id) + 1;

export const dbQueryCount = (ui: RenderedApp) => {
  let count = 0;
  const original = ui.memory.api.dbQuery;
  ui.memory.api.dbQuery = (args) => {
    count += 1;
    return original(args);
  };
  return () => count;
};

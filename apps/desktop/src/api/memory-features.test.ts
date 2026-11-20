import { describe, expect, it, vi } from "vitest";
import { LoroDoc, LoroList, LoroMap, LoroText } from "loro-crdt";
import type { CellValue, CoreEventMap, DbProperty, FilterNode, PreviewBlock } from "./contract.ts";
import { createMemoryCore } from "./memory.ts";
import { previewText } from "./memory-doc-ops.ts";

async function rejection(promise: Promise<unknown>): Promise<{ code: string; message: string }> {
  try {
    await promise;
  } catch (error) {
    return error as { code: string; message: string };
  }
  throw new Error("expected the call to reject");
}

async function localCore(options = {}) {
  const core = createMemoryCore({ syncDelayMs: 0, ...options });
  await core.api.workspaceCreateLocal();
  return core;
}

const text = (value: string): CellValue => ({ type: "text", value });

/** A database with a few typed properties and rows, for the query tests. */
async function sampleDatabase() {
  const core = await localCore();
  const { api } = core;
  const db = await api.dbCreate({ parentId: null, title: "Tasks" });
  const status = await api.dbPropertyAdd({ id: db, name: "Status", type: "select" });
  const open = await api.dbOptionAdd({ id: db, propertyId: status, name: "Open" });
  const done = await api.dbOptionAdd({ id: db, propertyId: status, name: "Done" });
  const tags = await api.dbPropertyAdd({ id: db, name: "Tags", type: "multi_select" });
  const red = await api.dbOptionAdd({ id: db, propertyId: tags, name: "Red", color: "red" });
  const blue = await api.dbOptionAdd({ id: db, propertyId: tags, name: "Blue", color: "blue" });
  const points = await api.dbPropertyAdd({ id: db, name: "Points", type: "number" });
  const due = await api.dbPropertyAdd({ id: db, name: "Due", type: "date" });
  const flag = await api.dbPropertyAdd({ id: db, name: "Flag", type: "checkbox" });
  const make = (title: string, values: Record<string, CellValue>) => api.dbRowCreate({ id: db, title, values });
  const rows = {
    a: await make("Đào", { [status]: { type: "select", value: open }, [points]: { type: "number", value: 5 }, [tags]: { type: "multi_select", value: [red] } }),
    b: await make("Dưa", { [status]: { type: "select", value: done }, [points]: { type: "number", value: 2 }, [due]: { type: "date", value: { start: "2026-11-05" } }, [flag]: { type: "checkbox", value: true } }),
    c: await make("Ăn", { [status]: { type: "select", value: open }, [tags]: { type: "multi_select", value: [red, blue] }, [due]: { type: "date", value: { start: "2026-11-20", end: "2026-11-24" } } }),
    d: await make("Bưởi", {}),
  };
  return { core, api, db, status, open, done, tags, red, blue, points, due, flag, rows };
}

const titlesOf = (result: { rows: { title: string }[] }) => result.rows.map((row) => row.title);

describe("in-memory core: databases", () => {
  it("keeps rows out of the tree snapshot, and only pages can be rows", async () => {
    const { api, db, rows } = await sampleDatabase();
    const snapshot = await api.treeSnapshot();
    expect(snapshot.map((node) => node.title)).toEqual(["Tasks"]);
    expect(snapshot[0]?.kind).toBe("database");
    expect((await api.pageOpen({ id: rows.a })).snapshot.byteLength).toBeGreaterThan(0);
    expect((await rejection(api.treeCreate({ kind: "folder", parentId: db }))).code).toBe("invalid_input");
    const loose = await api.treeCreate({ kind: "page", parentId: null, title: "Loose" });
    await api.treeMove({ id: loose, parentId: db });
    expect((await api.dbQuery({ id: db })).total).toBe(5);
    expect((await api.dbQuery({ id: db })).rows.at(-1)?.values).toEqual({});
    expect((await api.searchQuery({ text: "dua" })).map((hit) => hit.title)).toEqual(["Dưa"]);
  });

  it("filters with every operator family and nested and/or groups", async () => {
    const { api, db, status, open, tags, red, points, due, flag } = await sampleDatabase();
    const run = async (filter: FilterNode) => titlesOf(await api.dbQuery({ id: db, spec: { filter } }));
    expect(await run({ type: "condition", property: "title", operator: "contains", value: "dua" })).toEqual(["Dưa"]);
    expect(await run({ type: "condition", property: "title", operator: "starts_with", value: "an" })).toEqual(["Ăn"]);
    expect(await run({ type: "condition", property: status, operator: "is", value: open })).toEqual(["Đào", "Ăn"]);
    expect(await run({ type: "condition", property: status, operator: "is_empty" })).toEqual(["Bưởi"]);
    expect(await run({ type: "condition", property: tags, operator: "contains", value: red })).toEqual(["Đào", "Ăn"]);
    expect(await run({ type: "condition", property: points, operator: "gte", value: 2 })).toEqual(["Đào", "Dưa"]);
    expect(await run({ type: "condition", property: due, operator: "is", value: "2026-11-22" })).toEqual(["Ăn"]);
    expect(await run({ type: "condition", property: due, operator: "before", value: "2026-11-10" })).toEqual(["Dưa"]);
    expect(await run({ type: "condition", property: due, operator: "after", value: "2026-11-21" })).toEqual(["Ăn"]);
    expect(await run({ type: "condition", property: flag, operator: "is_checked" })).toEqual(["Dưa"]);
    expect(await run({ type: "condition", property: flag, operator: "is_unchecked" })).toEqual(["Đào", "Ăn", "Bưởi"]);
    expect(
      await run({
        type: "group",
        logic: "and",
        children: [
          { type: "condition", property: status, operator: "is", value: open },
          { type: "group", logic: "or", children: [{ type: "condition", property: points, operator: "gt", value: 4 }, { type: "condition", property: tags, operator: "contains", value: red }] },
        ],
      }),
    ).toEqual(["Đào", "Ăn"]);
    expect(await run({ type: "group", logic: "and", children: [] })).toHaveLength(4);
  });

  it("rejects a spec with an unknown property or an operator that does not suit the type", async () => {
    const { api, db, points } = await sampleDatabase();
    expect((await rejection(api.dbQuery({ id: db, spec: { filter: { type: "condition", property: "ghost", operator: "is", value: "x" } } }))).code).toBe("invalid_input");
    expect((await rejection(api.dbQuery({ id: db, spec: { filter: { type: "condition", property: points, operator: "contains", value: "1" } } }))).code).toBe("invalid_input");
    expect((await rejection(api.dbQuery({ id: db, spec: { group: { property: points, hideEmpty: false } } }))).code).toBe("invalid_input");
    const level = (children: FilterNode[]): FilterNode => ({ type: "group", logic: "and", children });
    const four = level([level([level([level([])])])]);
    expect((await rejection(api.dbQuery({ id: db, spec: { filter: four } }))).code).toBe("invalid_input");
  });

  it("sorts by several keys with the Vietnamese alphabet, empty values last, ties in manual order", async () => {
    const { api, db, status, points } = await sampleDatabase();
    expect(titlesOf(await api.dbQuery({ id: db, spec: { sorts: [{ property: "title", descending: false }] } }))).toEqual(["Ăn", "Bưởi", "Dưa", "Đào"]);
    expect(titlesOf(await api.dbQuery({ id: db, spec: { sorts: [{ property: "title", descending: true }] } }))).toEqual(["Đào", "Dưa", "Bưởi", "Ăn"]);
    expect(titlesOf(await api.dbQuery({ id: db, spec: { sorts: [{ property: points, descending: true }] } }))).toEqual(["Đào", "Dưa", "Ăn", "Bưởi"]);
    expect(titlesOf(await api.dbQuery({ id: db, spec: { sorts: [{ property: status, descending: false }, { property: points, descending: true }] } }))).toEqual(["Đào", "Ăn", "Dưa", "Bưởi"]);
  });

  it("groups rows into buckets in option order, with multi-select rows in each of their groups, and pages them", async () => {
    const { api, db, status, tags, open, done, red, blue } = await sampleDatabase();
    const byStatus = await api.dbQuery({ id: db, spec: { group: { property: status, hideEmpty: false } } });
    expect(byStatus.groups.map((group) => [group.key, group.count])).toEqual([[open, 2], [done, 1], [null, 1]]);
    expect(byStatus.rows).toEqual([]);
    const byTags = await api.dbQuery({ id: db, spec: { group: { property: tags, hideEmpty: true } } });
    expect(byTags.groups.map((group) => [group.key, group.count])).toEqual([[red, 2], [blue, 1], [null, 2]]);
    const paged = await api.dbQuery({ id: db, spec: { group: { property: status, hideEmpty: false }, limit: 1, offset: 1 } });
    expect(paged.groups.map((group) => group.rows.length)).toEqual([1, 0, 0]);
    const noneEmpty = await api.dbQuery({ id: db, spec: { filter: { type: "condition", property: status, operator: "is", value: open }, group: { property: status, hideEmpty: true } } });
    expect(noneEmpty.groups.map((group) => group.key)).toEqual([open]);
  });

  it("limits a date range query to rows whose date touches the range", async () => {
    const { api, db, due } = await sampleDatabase();
    const result = await api.dbQuery({ id: db, spec: { dateProperty: due, dateRange: { from: "2026-11-23", to: "2026-11-30" } } });
    expect(titlesOf(result)).toEqual(["Ăn"]);
  });

  it("converts on read, never rewrites cells, and creates options for text turned into a select", async () => {
    const { api, db, points } = await sampleDatabase();
    const note = await api.dbPropertyAdd({ id: db, name: "Note", type: "text" });
    const [first] = (await api.dbQuery({ id: db })).rows;
    await api.dbCellSet({ id: db, rowId: first?.id as string, propertyId: note, value: text("12.5") });
    await api.dbPropertyChangeType({ id: db, propertyId: note, type: "number" });
    expect((await api.dbQuery({ id: db })).rows[0]?.values[note]).toEqual({ type: "number", value: 12.5 });
    await api.dbPropertyChangeType({ id: db, propertyId: note, type: "text" });
    expect((await api.dbQuery({ id: db })).rows[0]?.values[note]).toEqual(text("12.5"));

    await api.dbCellSet({ id: db, rowId: first?.id as string, propertyId: note, value: text("Urgent") });
    await api.dbPropertyChangeType({ id: db, propertyId: note, type: "select" });
    const schema = await api.dbSchema({ id: db });
    const options = schema.properties.find((prop) => prop.id === note)?.options ?? [];
    expect(options.map((option) => option.name)).toEqual(["Urgent"]);
    expect((await api.dbQuery({ id: db })).rows[0]?.values[note]).toEqual({ type: "select", value: options[0]?.id });
    // The points column shows through a type that cannot hold numbers: empty, and back again.
    await api.dbPropertyChangeType({ id: db, propertyId: points, type: "date" });
    expect((await api.dbQuery({ id: db })).rows[0]?.values[points]).toBeUndefined();
    await api.dbPropertyChangeType({ id: db, propertyId: points, type: "number" });
    expect((await api.dbQuery({ id: db })).rows[0]?.values[points]).toEqual({ type: "number", value: 5 });
  });

  it("reads a cell as empty once its option is deleted, and refuses values of the wrong type", async () => {
    const { api, db, status, open, rows } = await sampleDatabase();
    await api.dbOptionDelete({ id: db, propertyId: status, optionId: open });
    expect((await api.dbQuery({ id: db })).rows.find((row) => row.id === rows.a)?.values[status]).toBeUndefined();
    expect((await rejection(api.dbCellSet({ id: db, rowId: rows.a, propertyId: status, value: text("x") }))).code).toBe("invalid_input");
    expect((await rejection(api.dbCellSet({ id: db, rowId: rows.a, propertyId: status, value: { type: "select", value: "ghost" } }))).code).toBe("not_found");
  });

  it("links rows both ways: writing a backlink edits the forward cells, and trashed rows leave the values", async () => {
    const { api } = await localCore();
    const projects = await api.dbCreate({ parentId: null, title: "Projects" });
    const tasks = await api.dbCreate({ parentId: null, title: "Tasks" });
    const forward = await api.dbRelationAdd({ id: tasks, name: "Project", targetId: projects, reverseName: "Tasks" });
    const back = (await api.dbSchema({ id: projects })).properties[0] as DbProperty;
    expect(back.relation).toMatchObject({ database: tasks, backlinkOf: forward });
    const p1 = await api.dbRowCreate({ id: projects, title: "Launch" });
    const t1 = await api.dbRowCreate({ id: tasks, title: "Write" });
    const t2 = await api.dbRowCreate({ id: tasks, title: "Ship" });
    await api.dbCellSet({ id: tasks, rowId: t1, propertyId: forward, value: { type: "relation", value: [p1] } });
    let launch = (await api.dbQuery({ id: projects })).rows[0];
    expect(launch?.values[back.id]).toEqual({ type: "relation", value: [t1] });
    expect((await api.dbQuery({ id: projects })).titles[t1]).toBe("Write");
    await api.dbCellSet({ id: projects, rowId: p1, propertyId: back.id, value: { type: "relation", value: [t1, t2] } });
    expect((await api.dbQuery({ id: tasks })).rows.map((row) => row.values[forward])).toEqual([{ type: "relation", value: [p1] }, { type: "relation", value: [p1] }]);
    await api.treeTrash({ id: t2 });
    launch = (await api.dbQuery({ id: projects })).rows[0];
    expect(launch?.values[back.id]).toEqual({ type: "relation", value: [t1] });
    expect((await api.dbQuery({ id: projects, spec: { filter: { type: "condition", property: back.id, operator: "contains", value: t1 } } })).total).toBe(1);
    await api.dbPropertyDelete({ id: tasks, propertyId: forward });
    expect((await api.dbSchema({ id: projects })).properties).toEqual([]);
  });

  it("moves a card to another group and position, and manages views", async () => {
    const { api, db, status, open, done, rows } = await sampleDatabase();
    await api.dbCardMove({ id: db, rowId: rows.a, propertyId: status, group: done, beforeId: rows.b });
    const board = await api.dbQuery({ id: db, spec: { group: { property: status, hideEmpty: false } } });
    expect(board.groups.find((group) => group.key === done)?.rows.map((row) => row.title)).toEqual(["Đào", "Dưa"]);
    await api.dbCardMove({ id: db, rowId: rows.b, propertyId: status, group: null });
    expect((await api.dbQuery({ id: db })).rows.map((row) => row.title)).toEqual(["Đào", "Ăn", "Bưởi", "Dưa"]);
    expect(open).toBeTruthy();

    const board2 = await api.dbViewAdd({ id: db, name: "Board", kind: "board" });
    let schema = await api.dbSchema({ id: db });
    expect(schema.views.map((view) => view.kind)).toEqual(["table", "board"]);
    expect(schema.views[1]?.group?.property).toBe(status);
    const view = { ...(schema.views[1] as NonNullable<(typeof schema.views)[number]>), name: "Kanban", sorts: [{ property: "title", descending: false }] };
    await api.dbViewSave({ id: db, view });
    await api.dbViewMove({ id: db, viewId: board2, index: 0 });
    schema = await api.dbSchema({ id: db });
    expect(schema.views.map((v) => v.name)).toEqual(["Kanban", "Table"]);
    expect(titlesOf(await api.dbQuery({ id: db, viewId: board2, spec: undefined }))).toEqual([]);
    await api.dbViewDelete({ id: db, viewId: board2 });
    expect((await rejection(api.dbViewDelete({ id: db, viewId: schema.views[1]?.id as string }))).code).toBe("invalid_input");
  });

  it("ignores saved conditions on deleted properties when a view is queried", async () => {
    const { api, db, points, rows } = await sampleDatabase();
    const schema = await api.dbSchema({ id: db });
    const view = { ...(schema.views[0] as NonNullable<(typeof schema.views)[number]>), filter: { type: "condition", property: points, operator: "gt", value: 100 } as const };
    await api.dbViewSave({ id: db, view });
    expect((await api.dbQuery({ id: db, viewId: view.id })).total).toBe(0);
    await api.dbPropertyDelete({ id: db, propertyId: points });
    expect((await api.dbQuery({ id: db, viewId: view.id })).total).toBe(4);
    expect(rows.a).toBeTruthy();
  });

  it("emits database:changed for local edits and only tree:changed for edits that arrive from another device", async () => {
    const { core, api, db, status, rows, open } = await sampleDatabase();
    const events: string[] = [];
    await core.events.on("database:changed", ({ databaseId, rowIds }) => events.push(`db:${databaseId === db}:${rowIds.length}`));
    await core.events.on("tree:changed", ({ nodeIds }) => events.push(`tree:${nodeIds.includes(db)}`));
    await api.dbCellSet({ id: db, rowId: rows.d, propertyId: status, value: { type: "select", value: open } });
    expect(events).toEqual(["db:true:1"]);
    events.length = 0;
    await core.control.receiveRemoteCell(db, rows.d, status, null);
    expect(events).toEqual(["tree:true"]);
  });

  it("pages through 10,000 rows in the manual order, with the total from the first page", async () => {
    const { core, api, db, points } = await sampleDatabase();
    await core.control.seedRows(db, 10_000, (at) => ({ title: `Row ${at}`, values: { [points]: { type: "number", value: at } } }));
    const first = await api.dbQuery({ id: db, offset: 0, limit: 200 });
    expect(first.total).toBe(10_004);
    expect(first.rows).toHaveLength(200);
    const far = await api.dbQuery({ id: db, offset: 9_900, limit: 200 });
    expect(far.rows).toHaveLength(104);
    const sorted = await api.dbQuery({ id: db, spec: { sorts: [{ property: points, descending: true }], limit: 3 } });
    expect(sorted.rows.map((row) => row.values[points])).toEqual([{ type: "number", value: 9_999 }, { type: "number", value: 9_998 }, { type: "number", value: 9_997 }]);
  }, 30_000);
});

describe("in-memory core: templates", () => {
  it("makes a page template from a page and starts new pages from it", async () => {
    const { api, control } = await localCore();
    const source = await api.treeCreate({ kind: "page", parentId: null, title: "Meeting" });
    const doc = control.pageDoc(source);
    const peer = new LoroDoc();
    peer.import((await api.pageOpen({ id: source })).snapshot);
    const paragraph = ((peer.getMap("doc").get("children") as LoroList).get(0) as LoroMap).get("children") as LoroList;
    paragraph.insertContainer(0, new LoroText()).insert(0, "Agenda");
    peer.commit();
    await api.pageApplyUpdate({ id: source, update: peer.export({ mode: "snapshot" }) });
    expect(doc).toBeDefined();
    const template = await api.templateCreateFromPage({ pageId: source, name: "Meeting notes" });
    expect((await api.templateList({})).map((t) => [t.name, t.kind])).toEqual([["Meeting notes", "page"]]);
    const made = await api.templateInstantiatePage({ id: template, parentId: null });
    const copy = new LoroDoc();
    copy.import((await api.pageOpen({ id: made })).snapshot);
    expect(JSON.stringify(copy.getMap("doc").toJSON())).toContain("Agenda");
    expect((await api.treeSnapshot()).find((node) => node.id === made)?.title).toBe("Meeting");
    expect((await api.templateInstantiatePage({ id: template, parentId: null, title: "Monday" }).then(() => api.treeSnapshot())).map((n) => n.title)).toContain("Monday");
    await api.templateRename({ id: template, name: "Notes" });
    expect((await api.templateList({}))[0]?.name).toBe("Notes");
    await api.templateDelete({ id: template });
    expect(await api.templateList({})).toEqual([]);
    expect((await rejection(api.templateDelete({ id: template }))).code).toBe("not_found");
  });

  it("gives new rows the default values of a row template and drops defaults that no longer fit", async () => {
    const { api, db, status, open, points } = await sampleDatabase();
    const template = await api.templateCreateForDatabase({ databaseId: db, name: "Bug" });
    await api.templateSetDefaults({ id: template, defaults: { [status]: { type: "select", value: open }, [points]: { type: "number", value: 3 } } });
    const row = await api.templateInstantiateRow({ id: template, title: "Crash" });
    const created = (await api.dbQuery({ id: db })).rows.find((r) => r.id === row);
    expect(created?.values[status]).toEqual({ type: "select", value: open });
    expect(created?.values[points]).toEqual({ type: "number", value: 3 });
    await api.dbOptionDelete({ id: db, propertyId: status, optionId: open });
    const again = await api.templateInstantiateRow({ id: template });
    expect((await api.dbQuery({ id: db })).rows.find((r) => r.id === again)?.values[status]).toBeUndefined();
    expect((await api.templateList({ databaseId: db })).map((t) => t.kind)).toEqual(["row"]);
    expect(await api.templateList({})).toEqual([]);
    const fromRow = await api.templateCreateForDatabase({ databaseId: db, name: "Copy of row", fromRowId: row });
    expect((await api.templateList({ databaseId: db })).find((t) => t.id === fromRow)?.defaults[points]).toEqual({ type: "number", value: 3 });
  });
});

describe("in-memory core: page history", () => {
  /** Edit a page as a device whose changes carry the given times (seconds). */
  const peers = new Map<string, LoroDoc>();
  async function editAt(core: Awaited<ReturnType<typeof localCore>>, pageId: string, at: number, textToAdd: string) {
    let peer = peers.get(pageId);
    if (!peer) {
      peer = new LoroDoc();
      peers.set(pageId, peer);
    }
    peer.import((await core.api.pageOpen({ id: pageId })).snapshot);
    peer.setRecordTimestamp(true);
    peer.setChangeMergeInterval(0);
    peer.setNextCommitTimestamp(at);
    const children = peer.getMap("doc").get("children") as LoroList;
    const paragraph = children.insertContainer(children.length, new LoroMap());
    paragraph.set("nodeName", "paragraph");
    paragraph.setContainer("attributes", new LoroMap());
    paragraph.setContainer("children", new LoroList()).insertContainer(0, new LoroText()).insert(0, textToAdd);
    peer.commit();
    await core.api.pageApplyUpdate({ id: pageId, update: peer.export({ mode: "snapshot" }) });
  }

  it("groups changes with gaps under five minutes and lists newest first", async () => {
    const core = await localCore();
    const page = await core.api.treeCreate({ kind: "page", parentId: null, title: "Log" });
    const base = 1_800_000_000;
    await editAt(core, page, base, "one");
    await editAt(core, page, base + 60, "two");
    await editAt(core, page, base + 3600, "three");
    const history = await core.api.pageHistory({ id: page });
    expect(history.trimmed).toBe(false);
    expect(history.previewable).toBe(true);
    const edited = history.versions.filter((version) => (version.startedAt ?? 0) >= base * 1000);
    expect(edited.map((version) => version.startedAt)).toEqual([(base + 3600) * 1000, base * 1000]);
    expect(edited[1]?.endedAt).toBe((base + 60) * 1000);
    expect(edited[1]?.changes).toBe(2);
    expect(edited[0]?.changes).toBe(1);
    expect(history.oldestAt).toBe(Math.min(...history.versions.map((version) => version.startedAt ?? Infinity)));
    expect(history.versions[0]?.changes).toBeGreaterThan(0);
  });

  it("previews a past version read-only and restores it as a new forward change delivered as a remote update", async () => {
    const core = await localCore();
    const page = await core.api.treeCreate({ kind: "page", parentId: null, title: "Log" });
    const base = 1_800_000_000;
    await editAt(core, page, base, "first draft");
    await editAt(core, page, base + 3600, "second thought");
    const versions = (await core.api.pageHistory({ id: page })).versions;
    const old = versions.find((version) => version.startedAt === base * 1000);
    expect(old).toBeDefined();
    const preview: PreviewBlock[] = await core.api.pageVersionPreview({ id: page, versionId: old?.id as string });
    expect(previewText(preview)).toContain("first draft");
    expect(previewText(preview)).not.toContain("second thought");
    const updates: CoreEventMap["page:remote-update"][] = [];
    await core.events.on("page:remote-update", (event) => updates.push(event));
    await core.api.pageVersionRestore({ id: page, versionId: old?.id as string });
    expect(updates).toHaveLength(1);
    expect(updates[0]?.pageId).toBe(page);
    const now = new LoroDoc();
    now.import((await core.api.pageOpen({ id: page })).snapshot);
    expect(JSON.stringify(now.getMap("doc").toJSON())).toContain("first draft");
    expect(JSON.stringify(now.getMap("doc").toJSON())).not.toContain("second thought");
    expect((await core.api.pageHistory({ id: page })).versions.length).toBeGreaterThan(versions.length - 1);
    expect((await rejection(core.api.pageVersionPreview({ id: page, versionId: "nope:1" }))).code).toBe("not_found");
  });

  it("says when older history is unavailable and then refuses to preview or restore", async () => {
    const core = await localCore();
    const page = await core.api.treeCreate({ kind: "page", parentId: null, title: "Log" });
    await editAt(core, page, 1_800_000_000, "x");
    core.control.setHistoryTrimmed(page, true);
    const history = await core.api.pageHistory({ id: page });
    expect(history).toMatchObject({ trimmed: true, previewable: false });
    expect((await rejection(core.api.pageVersionPreview({ id: page, versionId: history.versions[0]?.id as string }))).code).toBe("history_unavailable");
    expect((await rejection(core.api.pageVersionRestore({ id: page, versionId: history.versions[0]?.id as string }))).code).toBe("history_unavailable");
  });
});

describe("in-memory core: synced blocks", () => {
  async function pageWithBlocks(core: Awaited<ReturnType<typeof localCore>>, title: string, ...words: string[]) {
    const page = await core.api.treeCreate({ kind: "page", parentId: null, title });
    const doc = core.control.pageDoc(page) as LoroDoc;
    const children = doc.getMap("doc").get("children") as LoroList;
    children.delete(0, children.length);
    for (const word of words) {
      const paragraph = children.insertContainer(children.length, new LoroMap());
      paragraph.set("nodeName", "paragraph");
      paragraph.setContainer("attributes", new LoroMap());
      paragraph.setContainer("children", new LoroList()).insertContainer(0, new LoroText()).insert(0, word);
    }
    doc.commit();
    return page;
  }
  const names = (doc: LoroDoc) => ((doc.getMap("doc").get("children") as LoroList).toArray() as LoroMap[]).map((node) => String(node.get("nodeName")));

  it("turns blocks into shared content, shows it on another page, counts occurrences and unsyncs", async () => {
    const core = await localCore();
    const a = await pageWithBlocks(core, "A", "intro", "shared one", "shared two", "outro");
    const b = await pageWithBlocks(core, "B", "other");
    const updates: string[] = [];
    await core.events.on("page:remote-update", ({ pageId }) => updates.push(pageId));
    const { syncId } = await core.api.syncedCreate({ pageId: a, start: 1, end: 2 });
    expect(updates).toEqual([a]);
    expect(names(core.control.pageDoc(a) as LoroDoc)).toEqual(["paragraph", "synced_block", "paragraph"]);
    await core.api.syncedInsert({ pageId: b, parentPath: [], index: 1, syncId });
    expect(names(core.control.pageDoc(b) as LoroDoc)).toEqual(["paragraph", "synced_block"]);
    expect((await core.api.syncedOccurrences({ syncId })).map((o) => o.title).sort()).toEqual(["A", "B"]);
    expect(await core.api.syncedList()).toEqual([{ syncId, occurrences: 2, orphanedAt: null }]);
    const opened = new LoroDoc();
    opened.import((await core.api.syncedOpen({ syncId })).snapshot);
    expect(JSON.stringify(opened.getMap("doc").toJSON())).toContain("shared two");
    expect(opened.getMap("ne").get("role")).toBe("synced");

    await core.api.syncedUnsync({ pageId: b, path: [1] });
    expect(names(core.control.pageDoc(b) as LoroDoc)).toEqual(["paragraph", "paragraph", "paragraph"]);
    expect((await core.api.syncedOccurrences({ syncId })).map((o) => o.title)).toEqual(["A"]);
    await core.api.syncedUnsync({ pageId: a, path: [1] });
    expect(await core.api.syncedList()).toEqual([]);
    expect((await rejection(core.api.syncedOpen({ syncId }))).code).toBe("not_found");
  });

  it("refuses to nest shared content and keeps a block whose last page went away, starting the orphan clock", async () => {
    const core = await localCore({ now: () => 5000 });
    const a = await pageWithBlocks(core, "A", "x", "y");
    const { syncId } = await core.api.syncedCreate({ pageId: a, start: 0, end: 0 });
    expect((await rejection(core.api.syncedCreate({ pageId: a, start: 0, end: 1 }))).code).toBe("invalid_input");
    expect((await rejection(core.api.syncedCreate({ pageId: a, start: 0, end: 9 }))).code).toBe("invalid_input");
    await core.api.treeTrash({ id: a });
    expect((await core.api.syncedOccurrences({ syncId }))[0]).toMatchObject({ title: "A", trashed: true });
    expect((await core.api.syncedList())[0]?.orphanedAt).toBeNull();
    await core.api.treeDeletePermanently({ id: a });
    expect((await core.api.syncedList())[0]).toMatchObject({ occurrences: 0, orphanedAt: 5000 });
    expect((await core.api.syncedOpen({ syncId })).snapshot.byteLength).toBeGreaterThan(0);
  });
});

describe("in-memory core: reminders", () => {
  const offset = 7 * 60;

  it("lists reminders by due time, with snooze, done, reschedule and delete", async () => {
    const core = await localCore({ now: () => 1_000_000 });
    const page = await core.api.treeCreate({ kind: "page", parentId: null, title: "Pay rent" });
    const later = await core.api.reminderAdd({ pageId: page, at: 9_000_000, utcOffsetMinutes: offset, note: " call " });
    const sooner = await core.api.reminderAdd({ pageId: page, at: 5_000_000, utcOffsetMinutes: offset });
    expect(later.note).toBe("call");
    expect((await core.api.reminderList({})).map((e) => e.reminder.id)).toEqual([sooner.id, later.id]);
    await core.api.reminderSnooze({ id: sooner.id, until: 20_000_000 });
    const entries = await core.api.reminderList({});
    expect(entries.map((e) => e.reminder.id)).toEqual([later.id, sooner.id]);
    expect(entries[1]?.dueAt).toBe(20_000_000);
    await core.api.reminderSetDone({ id: later.id, done: true });
    expect((await core.api.reminderList({})).map((e) => e.reminder.id)).toEqual([sooner.id]);
    expect((await core.api.reminderList({ includeDone: true })).map((e) => e.reminder.done)).toContain(true);
    await core.api.reminderReschedule({ id: later.id, at: 7_000_000 });
    expect((await core.api.reminderList({}))[0]?.reminder.done).toBe(false);
    await core.api.reminderSetNote({ id: later.id, note: "x" });
    await core.api.reminderDelete({ id: later.id });
    expect((await core.api.reminderList({ includeDone: true })).map((e) => e.reminder.id)).toEqual([sooner.id]);
    expect((await rejection(core.api.reminderAdd({ pageId: page, utcOffsetMinutes: offset }))).code).toBe("invalid_input");
    await core.api.treeTrash({ id: page });
    expect((await core.api.reminderList({}))[0]?.pageVisible).toBe(false);
  });

  it("fires once when due, not for a moment older than ten minutes, never for a trashed page", async () => {
    const core = await localCore({ now: () => Date.parse("2026-11-21T08:00:00Z") });
    const page = await core.api.treeCreate({ kind: "page", parentId: null, title: "Stand-up" });
    const fired: CoreEventMap["reminder:due"][] = [];
    await core.events.on("reminder:due", (event) => fired.push(event));
    const at = Date.parse("2026-11-21T08:30:00Z");
    await core.api.reminderAdd({ pageId: page, at, utcOffsetMinutes: offset });
    core.control.checkReminders();
    expect(fired).toHaveLength(0);
    core.control.advanceClock(31 * 60_000);
    expect(fired).toHaveLength(1);
    expect(fired[0]).toMatchObject({ nativeShown: false });
    expect(fired[0]?.reminders[0]?.pageTitle).toBe("Stand-up");
    core.control.checkReminders();
    expect(fired).toHaveLength(1);

    const stale = await core.api.treeCreate({ kind: "page", parentId: null, title: "Old" });
    await core.api.reminderAdd({ pageId: stale, at: at - 3 * 3_600_000, utcOffsetMinutes: offset });
    const gone = await core.api.treeCreate({ kind: "page", parentId: null, title: "Gone" });
    await core.api.reminderAdd({ pageId: gone, at: at + 20 * 60_000, utcOffsetMinutes: offset });
    await core.api.treeTrash({ id: gone });
    core.control.advanceClock(5 * 60_000);
    await Promise.resolve();
    expect(fired).toHaveLength(1);
  });

  it("follows the date property of a row, reading a date without a time as 09:00 in the person's offset", async () => {
    const core = await localCore({ now: () => Date.parse("2026-11-19T00:00:00Z") });
    const db = await core.api.dbCreate({ parentId: null, title: "Tasks" });
    const due = await core.api.dbPropertyAdd({ id: db, name: "Due", type: "date" });
    const row = await core.api.dbRowCreate({ id: db, title: "Report", values: { [due]: { type: "date", value: { start: "2026-11-21" } } } });
    const reminder = await core.api.reminderAdd({ pageId: row, propertyId: due, utcOffsetMinutes: offset });
    expect((await core.api.reminderList({}))[0]?.dueAt).toBe(Date.parse("2026-11-21T02:00:00Z"));
    await core.api.dbCellSet({ id: db, rowId: row, propertyId: due, value: { type: "date", value: { start: "2026-11-22T10:15" } } });
    expect((await core.api.reminderList({}))[0]?.dueAt).toBe(Date.parse("2026-11-22T03:15:00Z"));
    expect(reminder.propertyId).toBe(due);
    const plain = await core.api.treeCreate({ kind: "page", parentId: null, title: "Plain" });
    expect((await rejection(core.api.reminderAdd({ pageId: plain, propertyId: due, utcOffsetMinutes: offset }))).code).toBe("invalid_input");
  });

  it("reports whether the core showed a native notification", async () => {
    const core = await localCore({ now: () => 1000, nativeReminders: true });
    const page = await core.api.treeCreate({ kind: "page", parentId: null, title: "P" });
    const fired = vi.fn();
    await core.events.on("reminder:due", fired);
    await core.api.reminderAdd({ pageId: page, at: 1000, utcOffsetMinutes: 0 });
    core.control.checkReminders();
    expect(fired).toHaveBeenCalledWith(expect.objectContaining({ nativeShown: true }));
  });
});

describe("in-memory core: structured import", () => {
  const file = (path: string, content: string | Uint8Array) => {
    const made = new File([content as BlobPart], path.split("/").pop() as string);
    Object.defineProperty(made, "webkitRelativePath", { value: path });
    return made;
  };

  it("imports folders, pages, CSV databases and pictures, and reports what it skipped", async () => {
    const files = [
      file("Export/Notes abcdefabcdefabcdefabcdefabcdefab/Plan abcdefabcdefabcdefabcdefabcdefab.md", "# Plan\nSee [Ideas](Ideas.md) and [Gone](Missing.md)\n![chart](chart.png)\n![lost](lost.png)\n"),
      file("Export/Notes abcdefabcdefabcdefabcdefabcdefab/Ideas.md", "Some ideas"),
      file("Export/Notes abcdefabcdefabcdefabcdefabcdefab/chart.png", new Uint8Array([137, 80, 78, 71])),
      file("Export/Tasks.csv", "Name,Points,Due\nWrite,3,2026-11-21\nShip,5,\n"),
      file("Export/Broken.csv", ""),
      file("Export/data.bin", "x"),
      file("Export/Tasks.csv", "dup"),
    ];
    const core = await localCore({ pickFiles: async () => files });
    const report = await core.api.importStructured({ parentId: null });
    expect(report).toMatchObject({ folders: 1, pages: 2, databases: 1, rows: 2, attachments: 1, linksRewritten: 1, linksUnresolved: 1, imagesMissing: 1 });
    expect(report?.skipped.map((reason) => reason.split(":")[0])).toEqual(["Broken.csv", "data.bin", "Tasks.csv"]);
    const tree = await core.api.treeSnapshot();
    expect(tree.map((node) => `${node.kind}:${node.title}`)).toEqual(["folder:Notes", "page:Plan", "page:Ideas", "database:Tasks"]);
    const database = tree.find((node) => node.kind === "database");
    const schema = await core.api.dbSchema({ id: database?.id as string });
    expect(schema.properties.map((prop) => [prop.name, prop.type])).toEqual([["Points", "number"], ["Due", "date"]]);
  });

  it("returns null when the picker is cancelled", async () => {
    const core = await localCore({ pickFiles: async () => [] });
    expect(await core.api.importStructured({ parentId: null })).toBeNull();
  });
});

describe("in-memory core: updates and recordings", () => {
  it("walks check, download and install, rejecting calls that do not fit the state", async () => {
    const core = await localCore();
    const states: string[] = [];
    await core.events.on("update:status", (status) => states.push(status.state));
    expect((await core.api.updateStatus()).state).toBe("idle");
    expect((await rejection(core.api.updateDownload())).code).toBe("no_update");
    expect((await core.api.updateCheck()).state).toBe("up_to_date");
    core.control.announceUpdate("1.2.3", "Fixes");
    const available = await core.api.updateCheck();
    expect(available).toMatchObject({ state: "available", version: "1.2.3", notes: "Fixes" });
    expect((await rejection(core.api.updateInstall())).code).toBe("update_not_ready");
    expect((await core.api.updateDownload()).state).toBe("ready");
    await core.api.updateInstall();
    expect(states).toEqual(["checking", "up_to_date", "checking", "available", "downloading", "ready"]);
  });

  it("returns the recording with the text only when keeping audio is on", async () => {
    const core = await localCore();
    await core.api.voiceDownloadModel({ name: "small-q5" });
    await core.api.voiceStart();
    expect((await core.api.voiceStop()).audio).toBeUndefined();
    await core.api.settingsSet({ settings: { voiceKeepAudio: true } });
    await core.api.voiceStart();
    const stopped = await core.api.voiceStop();
    expect(stopped.audio?.mime).toBe("audio/wav");
    expect(await core.api.attachmentUrl({ hash: stopped.audio?.hash as string })).toBeTruthy();
  });
});

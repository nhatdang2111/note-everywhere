import { LoroDoc } from "loro-crdt";
import { describe, expect, it } from "vitest";
import { CHANGE_MERGE_INTERVAL_SECONDS, configureDoc, createPageDoc, type PageDoc } from "./binding.ts";
import { createTestEditor, settle } from "../test/editor-harness.ts";

const changesOf = (doc: PageDoc | LoroDoc) => [...doc.getAllChanges().values()].flat();

describe("editor documents carry time", () => {
  it("records timestamps and merges changes closer than 30 seconds on a document the editor created", async () => {
    const t = await createTestEditor();
    t.setText("first");
    await settle();
    t.type(" second");
    await settle();
    const changes = changesOf(t.doc);
    expect(CHANGE_MERGE_INTERVAL_SECONDS).toBe(30);
    expect(changes.length).toBeGreaterThan(0);
    for (const change of changes) expect(change.timestamp).toBeGreaterThan(1_700_000_000);
    // Two typing bursts a few milliseconds apart are one change, not two.
    expect(changes).toHaveLength(1);
    t.destroy();
  });

  it("configures a document handed to the editor, such as one loaded from a snapshot", async () => {
    const loaded = new LoroDoc() as unknown as PageDoc;
    const t = await createTestEditor({ doc: loaded });
    t.setText("loaded");
    await settle();
    const changes = changesOf(loaded);
    expect(changes.every((change) => change.timestamp > 0)).toBe(true);
    t.destroy();
  });

  it("leaves an unconfigured document without times, which is why every editor document goes through configureDoc", () => {
    const plain = new LoroDoc();
    plain.getMap("doc").set("a", 1);
    plain.commit();
    expect(changesOf(plain)[0]?.timestamp).toBe(0);
    const configured = configureDoc(createPageDoc());
    (configured as unknown as LoroDoc).getMap("doc").set("a", 1);
    configured.commit();
    expect(changesOf(configured)[0]?.timestamp).toBeGreaterThan(0);
  });
});

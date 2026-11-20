import { LoroDoc } from "loro-crdt";
import { findAll, fold, matchesQuery } from "../lib/text.ts";
import type { AddedAttachment, Backlink, CoreApi, NodeId, SearchHit, TreeNode } from "./contract.ts";
import { apiError } from "./errors.ts";
import type { Bus, MemoryOptions } from "./memory-bus.ts";
import type { TreeStore } from "./memory-tree.ts";
import { appendParagraph, createSkeleton, pageLinks, plainText } from "./page-doc.ts";

type WorkspaceMethods = Pick<
  CoreApi,
  | "pageOpen"
  | "pageApplyUpdate"
  | "pageClose"
  | "pageBacklinks"
  | "searchQuery"
  | "attachmentAddBytes"
  | "attachmentPick"
  | "attachmentUrl"
  | "exportMarkdown"
  | "importMarkdown"
>;

export interface Workspace extends WorkspaceMethods {
  createPageDoc(node: TreeNode): void;
  dropPageDoc(id: NodeId): void;
  /** Merge an update that "arrived from another device" and notify the open editor. */
  receiveRemoteUpdate(pageId: NodeId, update: Uint8Array): void;
  /** The stored document, for assertions in tests. Created on first use for pages. */
  doc(id: NodeId): LoroDoc | undefined;
  /** Every page document that exists, trashed pages included. */
  allDocs(): ReadonlyMap<NodeId, LoroDoc>;
  /** Swap a page's document for another (templates). */
  replaceDoc(id: NodeId, doc: LoroDoc): void;
  /** The core changed a page's document: tell the open editor, as a remote update. */
  publishChange(pageId: NodeId, update: Uint8Array): void;
  /** Store bytes as an attachment. */
  addAttachment(bytes: Uint8Array, mime: string): Promise<AddedAttachment>;
  /** Pick files through the configured native-picker stand-in. */
  pickFiles(accept: string, multiple: boolean, directory?: boolean): Promise<File[]>;
}

const SNIPPET_RADIUS = 60;

function browserPick(accept: string, multiple: boolean, directory?: boolean): Promise<File[]> {
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = accept;
    input.multiple = multiple;
    if (directory) input.setAttribute("webkitdirectory", "");
    input.addEventListener("change", () => resolve([...(input.files ?? [])]));
    input.addEventListener("cancel", () => resolve([]));
    input.click();
  });
}

function browserSave(name: string, content: string): void {
  const url = URL.createObjectURL(new Blob([content], { type: "text/markdown" }));
  const link = document.createElement("a");
  link.href = url;
  link.download = name;
  link.click();
  URL.revokeObjectURL(url);
}

async function hashBytes(bytes: Uint8Array): Promise<string> {
  if (globalThis.crypto?.subtle) {
    const digest = await crypto.subtle.digest("SHA-256", bytes as BufferSource);
    return Array.from(new Uint8Array(digest), (b) => b.toString(16).padStart(2, "0")).join("");
  }
  let h = 2166136261;
  for (const byte of bytes) h = Math.imul(h ^ byte, 16777619) >>> 0;
  return h.toString(16).padStart(8, "0");
}

function toObjectUrl(bytes: Uint8Array, mime: string): string {
  if (typeof URL.createObjectURL === "function") return URL.createObjectURL(new Blob([bytes as BlobPart], { type: mime }));
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return `data:${mime};base64,${btoa(binary)}`;
}

function snippetAround(text: string, query: string): { snippet: string; highlights: [number, number][] } {
  const words = fold(query).split(/\s+/).filter(Boolean);
  const flat = text.replace(/\s+/g, " ").trim();
  const first = words.map((word) => fold(flat).indexOf(word)).filter((at) => at >= 0).sort((a, b) => a - b)[0] ?? 0;
  const start = Math.max(0, first - SNIPPET_RADIUS);
  const snippet = `${start > 0 ? "…" : ""}${flat.slice(start, start + SNIPPET_RADIUS * 2)}`;
  const highlights: [number, number][] = [];
  for (const word of words) {
    for (const at of findAll(snippet, word)) highlights.push([at, at + word.length]);
  }
  return { snippet, highlights: highlights.sort((a, b) => a[0] - b[0]) };
}

export function createWorkspace(tree: TreeStore, bus: Bus, options: MemoryOptions, markDirty: () => void): Workspace {
  const docs = new Map<NodeId, LoroDoc>();
  const attachments = new Map<string, { url: string; mime: string; byteLength: number }>();
  const pick = options.pickFiles ?? browserPick;
  const save = options.saveFile ?? browserSave;

  // Documents are made on first use: a database of thousands of rows does not pay for a document per row.
  const getDoc = (id: NodeId): LoroDoc | undefined => {
    const known = docs.get(id);
    if (known) return known;
    if (tree.get(id)?.kind !== "page") return undefined;
    const created = createSkeleton();
    docs.set(id, created);
    return created;
  };

  const needDoc = (id: NodeId): LoroDoc => {
    const doc = getDoc(id);
    if (!doc || !tree.isLive(id)) throw apiError("not_found", "That page is not available.");
    return doc;
  };

  async function addAttachment(bytes: Uint8Array, mime: string): Promise<AddedAttachment> {
    const hash = await hashBytes(bytes);
    const known = attachments.get(hash) ?? { url: toObjectUrl(bytes, mime), mime, byteLength: bytes.byteLength };
    attachments.set(hash, known);
    return { hash, mime, byteLength: known.byteLength, url: known.url };
  }

  const markdownOf = (id: NodeId): string => {
    const node = tree.get(id);
    return `# ${node?.title || "Untitled"}\n\n${plainText(getDoc(id) ?? createSkeleton())}\n`;
  };

  const subtreePages = (id: NodeId | null): NodeId[] =>
    tree.livePages().filter((page) => {
      if (id === null) return true;
      for (let cursor: NodeId | null = page.id; cursor !== null; cursor = tree.get(cursor)?.parentId ?? null) {
        if (cursor === id) return true;
      }
      return false;
    }).map((page) => page.id);

  return {
    createPageDoc: () => undefined,
    dropPageDoc: (id) => void docs.delete(id),
    doc: getDoc,
    allDocs: () => docs,
    replaceDoc: (id, doc) => void docs.set(id, doc),
    publishChange(pageId, update) {
      tree.touch(pageId);
      markDirty();
      bus.emit("page:remote-update", { pageId, update });
    },
    addAttachment,
    pickFiles: pick,

    async pageOpen({ id }) {
      return { snapshot: needDoc(id).export({ mode: "snapshot" }) };
    },
    async pageApplyUpdate({ id, update }) {
      try {
        needDoc(id).import(update);
      } catch {
        throw apiError("bad_update", "The page update could not be applied.");
      }
      tree.touch(id);
      markDirty();
    },
    async pageClose() {},
    receiveRemoteUpdate(pageId, update) {
      needDoc(pageId).import(update);
      bus.emit("page:remote-update", { pageId, update });
    },

    async pageBacklinks({ id }) {
      const links: Backlink[] = [];
      for (const page of tree.livePages()) {
        if (page.id === id) continue;
        const doc = docs.get(page.id);
        const ref = doc && pageLinks(doc).find((candidate) => candidate.pageId === id);
        if (ref) links.push({ pageId: page.id, title: page.title, snippet: ref.context });
      }
      return links;
    },

    async searchQuery({ text, limit = 20 }) {
      const query = text.trim();
      if (!query) return [];
      const hits: { hit: SearchHit; rank: number }[] = [];
      for (const page of tree.livePages()) {
        const body = plainText(docs.get(page.id) ?? createSkeleton());
        const inTitle = matchesQuery(page.title, query);
        if (!inTitle && !matchesQuery(body, query)) continue;
        const { snippet, highlights } = snippetAround(inTitle && !body ? page.title : body || page.title, query);
        hits.push({
          rank: inTitle ? 0 : 1,
          hit: { pageId: page.id, title: page.title, breadcrumb: tree.breadcrumb(page.id), snippet, highlights },
        });
      }
      return hits.sort((a, b) => a.rank - b.rank).slice(0, limit).map((entry) => entry.hit);
    },

    attachmentAddBytes: ({ bytes, mime }) => addAttachment(bytes, mime),
    async attachmentPick({ imagesOnly }) {
      const [file] = await pick(imagesOnly ? "image/*" : "*/*", false);
      if (!file) return null;
      return addAttachment(new Uint8Array(await file.arrayBuffer()), file.type || "application/octet-stream");
    },
    async attachmentUrl({ hash }) {
      const known = attachments.get(hash);
      if (!known) throw apiError("not_found", "That attachment is not stored on this device.");
      return known.url;
    },

    async exportMarkdown({ id }) {
      const ids = subtreePages(id);
      if (ids.length === 0) return { pagesWritten: 0 };
      const content = ids.map(markdownOf).join("\n---\n\n");
      save(id === null ? "workspace.md" : `${tree.get(id)?.title || "page"}.md`, content);
      return { pagesWritten: ids.length };
    },
    async importMarkdown({ parentId }) {
      const files = await pick(".md,.markdown,text/markdown", true);
      if (files.length === 0) return null;
      for (const file of files) {
        const id = await tree.treeCreate({ kind: "page", parentId, title: file.name.replace(/\.(md|markdown)$/i, "") });
        const doc = getDoc(id);
        if (!doc) continue;
        for (const line of (await file.text()).split(/\r?\n/)) if (line.trim()) appendParagraph(doc, line);
      }
      markDirty();
      return { pagesCreated: files.length };
    },
  };
}

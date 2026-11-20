import { isIsoDate } from "../lib/db/cell-convert.ts";
import type { AddedAttachment, CellValue, CoreApi, NodeId, PropertyType, StructuredImportReport } from "./contract.ts";
import type { DbStore } from "./memory-db.ts";
import { insertNode, rootChildren } from "./memory-doc-ops.ts";
import type { TreeStore } from "./memory-tree.ts";
import { parseCsv } from "./memory-vault.ts";
import { attachmentSrc } from "../editor/page-links.ts";
import type { LoroDoc } from "loro-crdt";
import { appendParagraph } from "./page-doc.ts";

export type StructuredImport = Pick<CoreApi, "importStructured">;

interface Hooks {
  tree: TreeStore;
  db: DbStore;
  doc(id: NodeId): LoroDoc | undefined;
  pickFiles(accept: string, multiple: boolean, directory?: boolean): Promise<File[]>;
  addAttachment(bytes: Uint8Array, mime: string): Promise<AddedAttachment>;
  markDirty(): void;
}

const MAX_FILE_BYTES = 10 * 1024 * 1024;
const IMAGE = /\.(png|jpe?g|gif|webp|svg)$/i;
/** Exports from other apps append a 32-character id to every file and folder name. */
const EXPORT_ID_SUFFIX = /\s+[0-9a-f]{32}$/i;

const cleanName = (segment: string): string => segment.replace(/\.[^.]+$/, "").replace(EXPORT_ID_SUFFIX, "").trim();

function relativeSegments(file: File): string[] {
  const path = file.webkitRelativePath || file.name;
  const segments = path.split("/").filter(Boolean);
  // The first segment is the folder the person picked.
  return segments.length > 1 ? segments.slice(1) : segments;
}

const decodeTarget = (target: string): string => {
  try {
    return decodeURIComponent(target);
  } catch {
    return target;
  }
};

const keyOf = (segments: string[]): string => segments.map((segment) => segment.replace(EXPORT_ID_SUFFIX, "")).join("/").toLowerCase();

function inferType(values: string[]): PropertyType {
  const filled = values.filter((value) => value.trim() !== "");
  if (filled.length === 0) return "text";
  if (filled.every((value) => Number.isFinite(Number(value)))) return "number";
  if (filled.every(isIsoDate)) return "date";
  return "text";
}

export function createStructuredImport(hooks: Hooks): StructuredImport {
  const { tree, db } = hooks;

  return {
    async importStructured({ parentId }) {
      const files = await hooks.pickFiles("", true, true);
      if (files.length === 0) return null;
      const report: StructuredImportReport = {
        folders: 0,
        pages: 0,
        databases: 0,
        rows: 0,
        attachments: 0,
        linksRewritten: 0,
        linksUnresolved: 0,
        imagesMissing: 0,
        skipped: [],
      };

      const folders = new Map<string, NodeId>();
      const folderFor = async (segments: string[]): Promise<NodeId | null> => {
        let parent = parentId;
        for (let depth = 1; depth < segments.length; depth += 1) {
          const key = segments.slice(0, depth).join("/");
          let id = folders.get(key);
          if (!id) {
            id = await tree.treeCreate({ kind: "folder", parentId: parent, title: (segments[depth - 1] ?? "").replace(EXPORT_ID_SUFFIX, "").trim() });
            folders.set(key, id);
            report.folders += 1;
          }
          parent = id;
        }
        return parent;
      };

      const attachments = new Map<string, string>();
      const pages: { id: NodeId; text: string; segments: string[] }[] = [];
      const seen = new Set<string>();

      for (const file of files) {
        const segments = relativeSegments(file);
        const label = segments.join("/");
        const key = keyOf(segments);
        if (seen.has(key)) {
          report.skipped.push(`${label}: duplicate of a file already imported`);
          continue;
        }
        seen.add(key);
        if (file.size > MAX_FILE_BYTES) {
          report.skipped.push(`${label}: larger than 10 MB`);
        } else if (/\.md$/i.test(file.name)) {
          const parent = await folderFor(segments);
          const id = await tree.treeCreate({ kind: "page", parentId: parent, title: cleanName(file.name) });
          pages.push({ id, text: await file.text(), segments });
          report.pages += 1;
        } else if (/\.csv$/i.test(file.name)) {
          const table = parseCsv(await file.text());
          const [header, ...body] = table;
          if (!header || header.length === 0 || header.every((cell) => cell.trim() === "")) {
            report.skipped.push(`${label}: not a CSV table`);
            continue;
          }
          const parent = await folderFor(segments);
          const databaseId = await db.dbCreate({ parentId: parent, title: cleanName(file.name) });
          const columns = header.slice(1).map((name, at) => {
            const type = inferType(body.map((row) => row[at + 1] ?? ""));
            return { type, propertyId: db.addProperty(databaseId, name.trim() || `Column ${at + 2}`, type) };
          });
          for (const row of body) {
            const values: Record<string, CellValue> = {};
            columns.forEach(({ type, propertyId }, at) => {
              const text = (row[at + 1] ?? "").trim();
              if (!text) return;
              if (type === "number") values[propertyId] = { type, value: Number(text) };
              else if (type === "date") values[propertyId] = { type, value: { start: text } };
              else values[propertyId] = { type: "text", value: text };
            });
            await db.dbRowCreate({ id: databaseId, title: row[0] ?? "", values });
            report.rows += 1;
          }
          report.databases += 1;
        } else if (IMAGE.test(file.name)) {
          const added = await hooks.addAttachment(new Uint8Array(await file.arrayBuffer()), file.type || "image/png");
          attachments.set(key, added.hash);
          report.attachments += 1;
        } else {
          report.skipped.push(`${label}: unsupported file type`);
        }
      }

      const pageKeys = new Set(pages.map((page) => keyOf(page.segments)));
      for (const page of pages) {
        const doc = hooks.doc(page.id);
        if (!doc) continue;
        const folder = page.segments.slice(0, -1);
        for (const line of page.text.split(/\r?\n/)) {
          const image = /^!\[([^\]]*)\]\(([^)]+)\)\s*$/.exec(line);
          if (image) {
            const hash = attachments.get(keyOf([...folder, ...decodeTarget(image[2] as string).split("/")]));
            if (hash) insertNode(rootChildren(doc), rootChildren(doc).length, "image", { src: attachmentSrc(hash), alt: image[1] ?? "" });
            else report.imagesMissing += 1;
            continue;
          }
          let text = line;
          for (const link of line.matchAll(/\[([^\]]*)\]\(([^)]+\.md)\)/gi)) {
            const target = keyOf([...folder, ...decodeTarget(link[2] as string).split("/")]);
            if (pageKeys.has(target)) report.linksRewritten += 1;
            else report.linksUnresolved += 1;
            text = text.replace(link[0], link[1] ?? "");
          }
          if (text.trim()) appendParagraph(doc, text);
        }
      }

      hooks.markDirty();
      return report;
    },
  };
}

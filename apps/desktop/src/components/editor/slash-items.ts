import type { BlockChoice } from "../../editor/index.ts";
import type { IconName } from "../common/icons.tsx";
import { fold } from "../../lib/text.ts";
import type { MessageKey } from "../../i18n/index.ts";

export interface SlashItem {
  id: BlockChoice;
  icon: IconName;
  label: MessageKey;
  hint: MessageKey;
  /** Extra words the filter also matches, including the Markdown shortcut. */
  keywords: string;
}

/** The 15 insertable block kinds, in the order of the product spec. */
export const SLASH_ITEMS: readonly SlashItem[] = [
  { id: "paragraph", icon: "paragraph", label: "block.paragraph", hint: "block.paragraphHint", keywords: "text plain paragraph van ban doan" },
  { id: "heading1", icon: "heading", label: "block.heading1", hint: "block.heading1Hint", keywords: "h1 heading title # tieu de" },
  { id: "heading2", icon: "heading", label: "block.heading2", hint: "block.heading2Hint", keywords: "h2 heading subtitle ## tieu de" },
  { id: "heading3", icon: "heading", label: "block.heading3", hint: "block.heading3Hint", keywords: "h3 heading ### tieu de" },
  { id: "bullet_list", icon: "bullet-list", label: "block.bulletList", hint: "block.bulletListHint", keywords: "ul bullet unordered list - danh sach" },
  { id: "ordered_list", icon: "number-list", label: "block.orderedList", hint: "block.orderedListHint", keywords: "ol numbered ordered list 1. danh sach so" },
  { id: "todo_list", icon: "checkbox", label: "block.todoList", hint: "block.todoListHint", keywords: "todo task checkbox check [] viec" },
  { id: "toggle", icon: "toggle", label: "block.toggle", hint: "block.toggleHint", keywords: "toggle collapse fold details gap" },
  { id: "quote", icon: "quote", label: "block.quote", hint: "block.quoteHint", keywords: "quote blockquote > trich dan" },
  { id: "callout", icon: "callout", label: "block.callout", hint: "block.calloutHint", keywords: "callout note info highlight luu y" },
  { id: "code_block", icon: "code", label: "block.codeBlock", hint: "block.codeBlockHint", keywords: "code snippet ``` ma" },
  { id: "divider", icon: "divider", label: "block.divider", hint: "block.dividerHint", keywords: "divider rule line hr --- ngan cach" },
  { id: "image", icon: "image", label: "block.image", hint: "block.imageHint", keywords: "image picture photo upload anh hinh" },
  { id: "child_page", icon: "page", label: "block.childPage", hint: "block.childPageHint", keywords: "page subpage child new trang con" },
  { id: "page_link", icon: "page-link", label: "block.pageLink", hint: "block.pageLinkHint", keywords: "link page mention reference lien ket trang" },
];

/**
 * Items matching what was typed after `/`: every word must appear in the
 * label or keywords, ignoring case and accents; items whose label starts with
 * the query come first.
 */
export function filterSlashItems(query: string, labelOf: (key: MessageKey) => string): SlashItem[] {
  const words = fold(query).split(/\s+/).filter(Boolean);
  if (words.length === 0) return [...SLASH_ITEMS];
  const scored: { item: SlashItem; rank: number }[] = [];
  for (const item of SLASH_ITEMS) {
    const label = fold(labelOf(item.label));
    const haystack = `${label} ${fold(item.keywords)} ${item.id.replace("_", " ")}`;
    if (!words.every((word) => haystack.includes(word))) continue;
    scored.push({ item, rank: label.startsWith(words[0] ?? "") ? 0 : 1 });
  }
  return scored.sort((a, b) => a.rank - b.rank).map((entry) => entry.item);
}

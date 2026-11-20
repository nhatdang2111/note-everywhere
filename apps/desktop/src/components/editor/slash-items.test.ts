import { describe, expect, it } from "vitest";
import { BLOCK_CHOICES } from "../../editor/index.ts";
import { en } from "../../i18n/en.ts";
import { filterSlashItems, SLASH_ITEMS } from "./slash-items.ts";

const label = (key: keyof typeof en) => en[key];
const ids = (query: string) => filterSlashItems(query, label).map((item) => item.id);

describe("slash menu filtering", () => {
  it("offers exactly the 15 block kinds of the product spec, once each", () => {
    expect(SLASH_ITEMS.map((item) => item.id)).toEqual([...BLOCK_CHOICES]);
    expect(new Set(SLASH_ITEMS.map((item) => item.id)).size).toBe(15);
    expect(ids("")).toHaveLength(15);
  });

  it("filters on the label", () => {
    expect(ids("head")).toEqual(["heading1", "heading2", "heading3"]);
    expect(ids("to-do")).toEqual(["todo_list"]);
    expect(ids("DIVIDER")).toEqual(["divider"]);
  });

  it("matches keywords and Markdown shortcuts", () => {
    expect(ids("h2")).toEqual(["heading2"]);
    expect(ids("###")).toEqual(["heading3"]);
    expect(ids("[]")).toEqual(["todo_list"]);
    expect(ids("---")).toEqual(["divider"]);
    expect(ids("```")).toEqual(["code_block"]);
    expect(ids(">")).toContain("quote");
  });

  it("ignores Vietnamese accents in what is typed", () => {
    expect(ids("tieu de")).toEqual(expect.arrayContaining(["heading1", "heading2", "heading3"]));
    expect(ids("TIÊU ĐỀ")).toEqual(expect.arrayContaining(["heading1"]));
  });

  it("needs every word to match", () => {
    expect(ids("list bulleted")).toEqual(["bullet_list"]);
    expect(ids("numbered list")).toEqual(["ordered_list"]);
    expect(ids("list zebra")).toEqual([]);
  });

  it("puts label prefixes before keyword hits", () => {
    const order = ids("page");
    expect(order).toEqual(expect.arrayContaining(["child_page", "page_link"]));
    expect(ids("link")[0]).toBe("page_link");
  });

  it("returns nothing for gibberish", () => {
    expect(ids("qqqq")).toEqual([]);
  });
});

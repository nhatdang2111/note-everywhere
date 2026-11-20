import type { CellValue, DateValue, DbProperty, PropertyType, SelectOption } from "../../api/contract.ts";
import { fold } from "../text.ts";

/**
 * How a cell reads when its property has a type other than the one it was
 * written with. The rules are the contract's conversion table; the core and
 * the in-memory test double both follow it, and the type-change confirmation
 * shows what it does to real cells.
 *
 * Property targets that appear for a source type are the ones that keep a
 * value; every other pair reads as empty.
 */
export const CONVERSION_TARGETS: Record<PropertyType, PropertyType[]> = {
  text: ["text", "number", "select", "multi_select", "date", "checkbox", "url"],
  url: ["url", "text", "number", "select", "multi_select", "date", "checkbox"],
  number: ["number", "text", "checkbox"],
  select: ["select", "text", "url", "multi_select"],
  multi_select: ["multi_select", "text", "url", "select"],
  date: ["date", "text"],
  checkbox: ["checkbox", "text", "number"],
  relation: ["relation"],
};

export type ConversionKind = "same" | "keeps" | "blank";

/** What changing a property from `from` to `to` does to existing cells. */
export function conversionKind(from: PropertyType, to: PropertyType): ConversionKind {
  if (from === to) return "same";
  return CONVERSION_TARGETS[from].includes(to) ? "keeps" : "blank";
}

const ISO_DAY = /^\d{4}-\d{2}-\d{2}(T\d{2}:\d{2})?$/;

export function isIsoDate(text: string): boolean {
  if (!ISO_DAY.test(text)) return false;
  const day = new Date(`${text.slice(0, 10)}T00:00:00Z`);
  return !Number.isNaN(day.getTime()) && day.toISOString().slice(0, 10) === text.slice(0, 10);
}

const optionNamed = (options: SelectOption[], name: string): SelectOption | undefined => {
  const wanted = fold(name.trim());
  return wanted ? options.find((option) => fold(option.name) === wanted) : undefined;
};

export function dateToText(value: DateValue): string {
  return value.end ? `${value.start} -> ${value.end}` : value.start;
}

function parseDate(text: string): DateValue | undefined {
  const parts = text.split("->").map((part) => part.trim());
  const [start, end] = parts;
  if (parts.length > 2 || !start || !isIsoDate(start)) return undefined;
  if (end === undefined) return { start };
  return isIsoDate(end) ? { start, end } : undefined;
}

/** Text a value reads as, for conversion to text or url. */
function asText(prop: DbProperty, raw: CellValue): string | undefined {
  switch (raw.type) {
    case "text":
    case "url":
      return raw.value;
    case "number":
      return String(raw.value);
    case "select":
      return prop.options.find((option) => option.id === raw.value)?.name;
    case "multi_select": {
      const names = raw.value.flatMap((id) => prop.options.find((option) => option.id === id)?.name ?? []);
      return names.length > 0 ? names.join(", ") : undefined;
    }
    case "date":
      return dateToText(raw.value);
    case "checkbox":
      return raw.value ? "true" : "false";
    case "relation":
      return undefined;
  }
}

/** A cell value of the property's type read from text, or undefined when the text does not fit. */
export function parseCellText(prop: DbProperty, text: string): CellValue | undefined {
  const trimmed = text.trim();
  if (prop.type === "text" || prop.type === "url") return text === "" ? undefined : { type: prop.type, value: text };
  if (trimmed === "") return undefined;
  switch (prop.type) {
    case "number": {
      const value = Number(trimmed);
      return Number.isFinite(value) ? { type: "number", value } : undefined;
    }
    case "select": {
      const option = optionNamed(prop.options, trimmed);
      return option ? { type: "select", value: option.id } : undefined;
    }
    case "multi_select": {
      const ids = new Set<string>();
      for (const part of trimmed.split(",")) {
        const option = optionNamed(prop.options, part);
        if (option) ids.add(option.id);
      }
      return ids.size > 0 ? { type: "multi_select", value: [...ids] } : undefined;
    }
    case "date": {
      const value = parseDate(trimmed);
      return value ? { type: "date", value } : undefined;
    }
    case "checkbox":
      return /^(true|yes|1|x)$/i.test(trimmed) ? { type: "checkbox", value: true } : undefined;
    case "relation":
      return undefined;
  }
}

/** Option names a text cell would need before it can read as a select or multi-select. */
export function optionNamesFor(text: string, to: "select" | "multi_select"): string[] {
  const names = to === "select" ? [text.trim()] : text.split(",").map((part) => part.trim());
  return names.filter(Boolean);
}

/** True when the value carries nothing to show. */
export function isEmptyCell(value: CellValue | undefined): boolean {
  if (!value) return true;
  switch (value.type) {
    case "text":
    case "url":
      return value.value === "";
    case "multi_select":
    case "relation":
      return value.value.length === 0;
    default:
      return false;
  }
}

/**
 * The value a stored cell has through `prop`'s current type (undefined reads
 * as an empty cell). Stored values never change; only this reading does, so
 * changing a type back restores them.
 */
export function readCell(prop: DbProperty, raw: CellValue | undefined): CellValue | undefined {
  if (!raw) return undefined;
  const known = (id: string) => prop.options.some((option) => option.id === id);
  let result: CellValue | undefined;
  if (raw.type === prop.type) {
    if (raw.type === "select") result = known(raw.value) ? raw : undefined;
    else if (raw.type === "multi_select") result = { type: "multi_select", value: raw.value.filter(known) };
    else result = raw;
  } else if (conversionKind(raw.type, prop.type) === "blank") {
    result = undefined;
  } else if (prop.type === "text" || prop.type === "url") {
    const text = asText(prop, raw);
    result = text === undefined ? undefined : { type: prop.type, value: text };
  } else if (raw.type === "number" && prop.type === "checkbox") {
    result = raw.value !== 0 ? { type: "checkbox", value: true } : undefined;
  } else if (raw.type === "checkbox" && prop.type === "number") {
    result = { type: "number", value: raw.value ? 1 : 0 };
  } else if (raw.type === "select" && prop.type === "multi_select") {
    result = known(raw.value) ? { type: "multi_select", value: [raw.value] } : undefined;
  } else if (raw.type === "multi_select" && prop.type === "select") {
    const first = raw.value.find(known);
    result = first === undefined ? undefined : { type: "select", value: first };
  } else if (raw.type === "text" || raw.type === "url") {
    result = parseCellText(prop, raw.value);
  }
  return isEmptyCell(result) ? undefined : result;
}

/** Text shown or copied for a value; relations use `titles` for the rows they point at. */
export function cellToText(prop: DbProperty, value: CellValue | undefined, titles: Record<string, string> = {}): string {
  if (!value) return "";
  if (value.type === "relation") return value.value.flatMap((id) => titles[id] ?? []).join(", ");
  if (value.type === "checkbox") return value.value ? "true" : "";
  return asText(prop, value) ?? "";
}

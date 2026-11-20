import type { FilterOperator, OptionColor, PropertyType } from "../../api/contract.ts";

/** The title behaves like a text property wherever operators are chosen. */
export type FilterableType = PropertyType | "title";

const TEXT_OPERATORS: FilterOperator[] = ["contains", "not_contains", "is", "is_not", "starts_with", "ends_with", "is_empty", "is_not_empty"];

/** Which operators suit which property type, exactly as the contract lists them. */
export const OPERATORS: Record<FilterableType, FilterOperator[]> = {
  title: TEXT_OPERATORS,
  text: TEXT_OPERATORS,
  url: TEXT_OPERATORS,
  number: ["is", "is_not", "gt", "gte", "lt", "lte", "is_empty", "is_not_empty"],
  select: ["is", "is_not", "is_empty", "is_not_empty"],
  multi_select: ["contains", "not_contains", "is_empty", "is_not_empty"],
  date: ["is", "before", "after", "on_or_before", "on_or_after", "is_empty", "is_not_empty"],
  checkbox: ["is_checked", "is_unchecked"],
  relation: ["contains", "not_contains", "is_empty", "is_not_empty"],
};

const VALUELESS = new Set<FilterOperator>(["is_empty", "is_not_empty", "is_checked", "is_unchecked"]);

/** Operators that take no value. */
export const needsValue = (operator: FilterOperator): boolean => !VALUELESS.has(operator);

export const operatorFits = (type: FilterableType, operator: FilterOperator): boolean => OPERATORS[type].includes(operator);

export const OPTION_COLORS: OptionColor[] = ["gray", "brown", "orange", "yellow", "green", "blue", "purple", "pink", "red"];

/** The colour a new option gets: they cycle so neighbours differ. */
export const nextOptionColor = (existing: number): OptionColor => OPTION_COLORS[(existing + 1) % OPTION_COLORS.length] as OptionColor;

export const PROPERTY_TYPES: PropertyType[] = ["text", "number", "select", "multi_select", "date", "checkbox", "url", "relation"];

/** Property types a board can group by. */
export const GROUPABLE: PropertyType[] = ["select", "multi_select", "checkbox"];

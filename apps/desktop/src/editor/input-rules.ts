import { InputRule, inputRules, textblockTypeInputRule, wrappingInputRule } from "prosemirror-inputrules";
import type { Plugin } from "prosemirror-state";
import { pageSchema } from "./schema.ts";

const { nodes, marks } = pageSchema;

/** `---` turns its paragraph into a divider and leaves an empty paragraph after it. */
const dividerRule = new InputRule(/^---$/, (state, _match, start) => {
  const $start = state.doc.resolve(start);
  return state.tr.replaceWith($start.before(), $start.after(), [nodes.divider.create(), nodes.paragraph.create()]);
});

/**
 * Inline mark rule for `**x**`, `*x*`, `` `x` `` and `~~x~~`. The closing
 * delimiter is the character being typed, so it is not in the document yet.
 */
function markRule(pattern: RegExp, markName: keyof typeof marks): InputRule {
  return new InputRule(pattern, (state, match, start, end) => {
    const text = match[1];
    const mark = marks[markName];
    return state.tr
      .delete(start, end)
      .insertText(text, start)
      .addMark(start, start + text.length, mark.create())
      .removeStoredMark(mark);
  });
}

export function pageInputRules(): Plugin {
  return inputRules({
    rules: [
      textblockTypeInputRule(/^(#{1,3})\s$/, nodes.heading, (match) => ({ level: match[1].length })),
      textblockTypeInputRule(/^```$/, nodes.code_block),
      wrappingInputRule(/^\s*[-+*]\s$/, nodes.bullet_list),
      wrappingInputRule(/^\d+\.\s$/, nodes.ordered_list),
      wrappingInputRule(/^\[\s?\]\s$/, nodes.todo_list),
      wrappingInputRule(/^>\s$/, nodes.quote),
      dividerRule,
      markRule(/\*\*([^*]+)\*\*$/, "bold"),
      markRule(/(?<!\*)\*([^*]+)\*$/, "italic"),
      markRule(/`([^`]+)`$/, "code"),
      markRule(/~~([^~]+)~~$/, "strike"),
    ],
  });
}

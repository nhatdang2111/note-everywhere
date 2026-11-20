import { For, Match, Show, Switch, type JSX } from "solid-js";
import type { PreviewBlock, PreviewInline } from "../../api/contract.ts";
import { t } from "../../i18n/index.ts";

function Inline(props: { part: PreviewInline }): JSX.Element {
  const part = () => props.part;
  return (
    <Show when={part().type === "text" && (part() as Extract<PreviewInline, { type: "text" }>)} fallback={<br />}>
      {(text) => {
        let node: JSX.Element = <>{text().text}</>;
        const { marks } = text();
        if (marks.code) node = <code>{node}</code>;
        if (marks.strike) node = <s>{node}</s>;
        if (marks.italic) node = <em>{node}</em>;
        if (marks.bold) node = <strong>{node}</strong>;
        if (marks.link) node = <span class="preview-link">{node}</span>;
        return node;
      }}
    </Show>
  );
}

function Content(props: { block: PreviewBlock }): JSX.Element {
  return (
    <>
      <For each={props.block.inline}>{(part) => <Inline part={part} />}</For>
      <For each={props.block.children}>{(child) => <Block block={child} />}</For>
    </>
  );
}

/** A read-only rendering of one block of a past version. */
export function Block(props: { block: PreviewBlock }): JSX.Element {
  const kind = () => props.block.kind;
  return (
    <Switch fallback={<div class="preview-unknown">{t("history.unknownBlock")}</div>}>
      <Match when={kind().type === "paragraph"}><p><Content block={props.block} /></p></Match>
      <Match when={kind().type === "heading" && (kind() as { level: number }).level}>
        {(level) => (
          <Switch>
            <Match when={level() === 1}><h3><Content block={props.block} /></h3></Match>
            <Match when={level() === 2}><h4><Content block={props.block} /></h4></Match>
            <Match when={true}><h5><Content block={props.block} /></h5></Match>
          </Switch>
        )}
      </Match>
      <Match when={kind().type === "bullet_list"}><ul><Content block={props.block} /></ul></Match>
      <Match when={kind().type === "ordered_list"}><ol><Content block={props.block} /></ol></Match>
      <Match when={kind().type === "todo_list"}><ul class="preview-todos"><Content block={props.block} /></ul></Match>
      <Match when={kind().type === "list_item"}><li><Content block={props.block} /></li></Match>
      <Match when={kind().type === "todo_item"}>
        <li data-checked={(kind() as { checked: boolean }).checked ? "true" : "false"}>
          <span aria-label={t((kind() as { checked: boolean }).checked ? "history.done" : "history.notDone")}>{(kind() as { checked: boolean }).checked ? "☑" : "☐"}</span> <Content block={props.block} />
        </li>
      </Match>
      <Match when={kind().type === "toggle"}><details open={(kind() as { open: boolean }).open}><summary><Content block={props.block} /></summary></details></Match>
      <Match when={kind().type === "quote"}><blockquote><Content block={props.block} /></blockquote></Match>
      <Match when={kind().type === "callout"}><div class="preview-callout"><span aria-hidden="true">{(kind() as { icon: string }).icon}</span><div><Content block={props.block} /></div></div></Match>
      <Match when={kind().type === "divider"}><hr /></Match>
      <Match when={kind().type === "code_block"}><pre><code><Content block={props.block} /></code></pre></Match>
      <Match when={kind().type === "image"}><div class="preview-unknown">{t("history.image", { alt: (kind() as { alt: string }).alt })}</div></Match>
      <Match when={kind().type === "child_page" || kind().type === "page_link"}><div class="preview-card">{(kind() as { title: string }).title || t("tree.untitled")}</div></Match>
      <Match when={kind().type === "synced_block"}><div class="preview-card">{t("history.syncedBlock")}</div></Match>
    </Switch>
  );
}

export function PreviewBlocks(props: { blocks: PreviewBlock[] }): JSX.Element {
  return <div class="preview-blocks"><For each={props.blocks}>{(block) => <Block block={block} />}</For></div>;
}

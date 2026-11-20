import { Show, createSignal, type JSX } from "solid-js";
import type { VaultItemInput, VaultItemKind, VaultItemSummary } from "../../api/contract.ts";
import { t, type MessageKey } from "../../i18n/index.ts";
import { createSubmit, invalid } from "../../lib/use-submit.ts";
import { useApp } from "../../state/app.tsx";
import { Button } from "../common/button.tsx";
import { PasswordField, TextField } from "../common/form.tsx";
import { ErrorBanner } from "../common/states.tsx";
import { GeneratorDialog } from "./vault-dialogs.tsx";

export const KIND_LABEL: Record<VaultItemKind, MessageKey> = {
  login: "vault.kindLogin",
  secure_note: "vault.kindNote",
  api_key: "vault.kindApiKey",
};

const lines = (text: string) =>
  text
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean);
const tagsOf = (text: string) => [...new Set(text.split(",").map((tag) => tag.trim()).filter(Boolean))];

export interface ItemEditorProps {
  /** Present when editing; absent when creating. */
  item?: VaultItemSummary;
  kind: VaultItemKind;
  onSaved(item: VaultItemSummary): void;
  onCancel(): void;
}

/**
 * Create or edit one vault item. Existing secrets are never loaded into the
 * form: a blank secret box means "keep what is stored", typing replaces it.
 */
export function ItemEditor(props: ItemEditorProps): JSX.Element {
  const { vault } = useApp();
  // The pane that shows this form swaps it out when the selection changes; read props once.
  const item = props.item;
  const kind = props.kind;
  const editing = () => item !== undefined;
  const [title, setTitle] = createSignal(item?.title ?? "");
  const [username, setUsername] = createSignal(item?.username ?? "");
  const [password, setPassword] = createSignal("");
  const [secretValue, setSecretValue] = createSignal("");
  const [urls, setUrls] = createSignal((item?.urls ?? []).join("\n"));
  const [notes, setNotes] = createSignal("");
  const [tags, setTags] = createSignal((item?.tags ?? []).join(", "));
  const [generating, setGenerating] = createSignal(false);

  const form = createSubmit(async () => {
    if (!title().trim()) throw invalid(t("vault.titleRequired"));
    const input: VaultItemInput = { id: item?.id, kind: kind, title: title().trim(), urls: lines(urls()), tags: tagsOf(tags()) };
    if (kind === "login") {
      input.username = username().trim();
      if (password()) input.password = password();
    }
    if (kind === "api_key" && secretValue()) input.value = secretValue();
    if (notes()) input.notes = notes();
    props.onSaved(await vault.save(input));
  });

  return (
    <form class="form item-editor" onSubmit={form.submit} aria-label={editing() ? t("vault.editItem") : t("vault.newItem")}>
      <h2>{editing() ? t("vault.editItem") : t("vault.newKind", { kind: t(KIND_LABEL[kind]) })}</h2>
      <TextField label={t("vault.titleLabel")} value={title()} onInput={setTitle} autofocus />
      <Show when={kind === "login"}>
        <TextField label={t("vault.username")} value={username()} onInput={setUsername} autocomplete="off" />
        <div class="password-with-tool">
          <PasswordField
            label={t("vault.itemPassword")}
            value={password()}
            onInput={setPassword}
            hint={editing() ? t("vault.keepBlank") : undefined}
            autocomplete="new-password"
          />
          <Button icon="dice" onClick={() => setGenerating(true)}>
            {t("generator.open")}
          </Button>
        </div>
      </Show>
      <Show when={kind === "api_key"}>
        <PasswordField label={t("vault.apiKeyValue")} value={secretValue()} onInput={setSecretValue} hint={editing() ? t("vault.keepBlank") : undefined} autocomplete="off" />
      </Show>
      <div class="field">
        <label for="item-urls">{t("vault.urls")}</label>
        <textarea id="item-urls" rows="2" value={urls()} onInput={(event) => setUrls(event.currentTarget.value)} />
        <p class="field-hint">{t("vault.urlsHint")}</p>
      </div>
      <div class="field">
        <label for="item-notes">{t("vault.notes")}</label>
        <textarea id="item-notes" rows="4" value={notes()} onInput={(event) => setNotes(event.currentTarget.value)} />
        <Show when={editing()}>
          <p class="field-hint">{t("vault.keepBlank")}</p>
        </Show>
      </div>
      <TextField label={t("vault.tags")} value={tags()} onInput={setTags} hint={t("vault.tagsHint")} />
      <ErrorBanner error={form.error()} />
      <div class="form-actions">
        <Button onClick={props.onCancel}>{t("common.cancel")}</Button>
        <Button variant="primary" type="submit" busy={form.busy()}>
          {t("common.save")}
        </Button>
      </div>
      <Show when={generating()}>
        <GeneratorDialog
          onClose={() => setGenerating(false)}
          onUse={(value) => {
            setPassword(value);
            setGenerating(false);
          }}
        />
      </Show>
    </form>
  );
}

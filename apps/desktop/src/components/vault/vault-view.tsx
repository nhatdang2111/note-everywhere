import { For, Match, Show, Switch, createSignal, type JSX } from "solid-js";
import type { VaultItemKind, VaultItemSummary } from "../../api/contract.ts";
import { t } from "../../i18n/index.ts";
import { formatRelative } from "../../lib/format.ts";
import { useApp } from "../../state/app.tsx";
import { RecoveryKeyPanel } from "../auth/recovery-key-panel.tsx";
import { Button } from "../common/button.tsx";
import { ConfirmDialog } from "../common/confirm-dialog.tsx";
import { Icon } from "../common/icons.tsx";
import { Menu } from "../common/menu.tsx";
import { EmptyState, ErrorBanner, Spinner } from "../common/states.tsx";
import { ItemEditor, KIND_LABEL } from "./item-editor.tsx";
import { SecretRow } from "./secret-row.tsx";
import { ChangeVaultPasswordDialog, CsvImportDialog, GeneratorDialog } from "./vault-dialogs.tsx";
import { VaultSetup, VaultUnlock } from "./vault-gate.tsx";

type Pane = { mode: "none" } | { mode: "view"; id: string } | { mode: "new"; kind: VaultItemKind } | { mode: "edit"; id: string };
type Dialogs = "password" | "generator" | "csv" | null;

const KIND_ICON = { login: "key", secure_note: "page", api_key: "code" } as const;

function ItemDetail(props: { item: VaultItemSummary; onEdit(): void; onDelete(): void }): JSX.Element {
  return (
    <section class="item-detail" aria-label={props.item.title}>
      <header class="view-header">
        <h2>{props.item.title}</h2>
        <div class="header-actions">
          <Button icon="edit" onClick={props.onEdit}>
            {t("common.edit")}
          </Button>
          <Button variant="danger" icon="trash" onClick={props.onDelete}>
            {t("common.delete")}
          </Button>
        </div>
      </header>
      <p class="muted">
        {t(KIND_LABEL[props.item.kind])} · {t("vault.updated", { when: formatRelative(props.item.updatedAt) })}
      </p>
      <Show when={props.item.username}>
        <div class="detail-row">
          <span class="secret-label">{t("vault.username")}</span>
          <span>{props.item.username}</span>
        </div>
      </Show>
      <Show when={props.item.urls.length > 0}>
        <div class="detail-row">
          <span class="secret-label">{t("vault.urls")}</span>
          <ul class="url-list">
            <For each={props.item.urls}>{(url) => <li>{url}</li>}</For>
          </ul>
        </div>
      </Show>
      <Show when={props.item.kind === "login"}>
        <SecretRow itemId={props.item.id} field="password" label={t("vault.itemPassword")} />
      </Show>
      <Show when={props.item.kind === "api_key"}>
        <SecretRow itemId={props.item.id} field="value" label={t("vault.apiKeyValue")} />
      </Show>
      <SecretRow itemId={props.item.id} field="notes" label={t("vault.notes")} />
      <Show when={props.item.tags.length > 0}>
        <ul class="tag-list" aria-label={t("vault.tags")}>
          <For each={props.item.tags}>{(tag) => <li class="tag">{tag}</li>}</For>
        </ul>
      </Show>
    </section>
  );
}

function UnlockedVault(props: { sameAsAccount: boolean; onDismissWarning(): void }): JSX.Element {
  const app = useApp();
  const { vault } = app;
  const [pane, setPane] = createSignal<Pane>({ mode: "none" });
  const [dialog, setDialog] = createSignal<Dialogs>(null);
  const [newMenu, setNewMenu] = createSignal<{ x: number; y: number } | null>(null);
  const [deleting, setDeleting] = createSignal<VaultItemSummary | null>(null);

  const selected = () => {
    const current = pane();
    return current.mode === "view" || current.mode === "edit" ? vault.items().find((item) => item.id === current.id) : undefined;
  };

  return (
    <div class="vault-layout">
      <aside class="vault-list-pane" aria-label={t("vault.listLabel")}>
        <div class="vault-toolbar">
          <input
            type="search"
            aria-label={t("vault.search")}
            placeholder={t("vault.search")}
            value={vault.query()}
            onInput={(event) => vault.setQuery(event.currentTarget.value)}
          />
          <Button
            variant="primary"
            icon="plus"
            onClick={(event) => {
              const rect = event.currentTarget.getBoundingClientRect();
              setNewMenu({ x: rect.left, y: rect.bottom + 4 });
            }}
          >
            {t("vault.new")}
          </Button>
        </div>
        <Show when={vault.tags().length > 0}>
          <div class="tag-filter" role="group" aria-label={t("vault.filterByTag")}>
            <For each={vault.tags()}>
              {(tag) => (
                <button type="button" class="tag tag-button" aria-pressed={vault.activeTag() === tag} onClick={() => vault.setActiveTag(vault.activeTag() === tag ? null : tag)}>
                  {tag}
                </button>
              )}
            </For>
          </div>
        </Show>
        <ErrorBanner error={vault.loadError()} />
        <Show
          when={vault.visibleItems().length > 0}
          fallback={
            <EmptyState icon="key" title={vault.query() || vault.activeTag() ? t("vault.noMatches") : t("vault.emptyTitle")}>
              {vault.query() || vault.activeTag() ? t("vault.noMatchesBody") : t("vault.emptyBody")}
            </EmptyState>
          }
        >
          <ul class="vault-list">
            <For each={vault.visibleItems()}>
              {(item) => (
                <li>
                  <button
                    type="button"
                    class="vault-item"
                    aria-current={selected()?.id === item.id ? "true" : undefined}
                    onClick={() => setPane({ mode: "view", id: item.id })}
                  >
                    <Icon name={KIND_ICON[item.kind]} />
                    <span class="vault-item-text">
                      <span class="vault-item-title">{item.title}</span>
                      <Show when={item.username}>
                        <span class="vault-item-sub">{item.username}</span>
                      </Show>
                    </span>
                  </button>
                </li>
              )}
            </For>
          </ul>
        </Show>
      </aside>

      <section class="vault-detail-pane">
        <header class="vault-actions">
          <Show when={props.sameAsAccount}>
            <div class="warning-banner" role="alert">
              <Icon name="alert" />
              <span>{t("vault.samePasswordWarning")}</span>
              <Button onClick={() => setDialog("password")}>{t("vault.changePassword")}</Button>
              <Button variant="ghost" onClick={props.onDismissWarning}>
                {t("common.dismiss")}
              </Button>
            </div>
          </Show>
          <div class="header-actions">
            <Button icon="dice" onClick={() => setDialog("generator")}>
              {t("generator.open")}
            </Button>
            <Button icon="download" onClick={() => setDialog("csv")}>
              {t("csv.title")}
            </Button>
            <Button icon="key" onClick={() => setDialog("password")}>
              {t("vault.changePassword")}
            </Button>
            <Button icon="lock" onClick={() => void app.guard(() => vault.lock())}>
              {t("vault.lock")}
            </Button>
          </div>
        </header>
        <Switch fallback={<EmptyState icon="lock" title={t("vault.pickTitle")}>{t("vault.pickBody")}</EmptyState>}>
          <Match when={pane().mode === "new" && (pane() as { kind: VaultItemKind })}>
            {(current) => <ItemEditor kind={current().kind} onCancel={() => setPane({ mode: "none" })} onSaved={(saved) => setPane({ mode: "view", id: saved.id })} />}
          </Match>
          <Match when={pane().mode === "edit" && selected()}>
            {(item) => <ItemEditor item={item()} kind={item().kind} onCancel={() => setPane({ mode: "view", id: item().id })} onSaved={(saved) => setPane({ mode: "view", id: saved.id })} />}
          </Match>
          <Match when={pane().mode === "view" && selected()}>
            {(item) => <ItemDetail item={item()} onEdit={() => setPane({ mode: "edit", id: item().id })} onDelete={() => setDeleting(item())} />}
          </Match>
        </Switch>
      </section>

      <Show when={newMenu()}>
        {(at) => (
          <Menu
            label={t("vault.new")}
            at={at()}
            onClose={() => setNewMenu(null)}
            entries={(["login", "secure_note", "api_key"] as const).map((kind) => ({
              kind: "item" as const,
              icon: KIND_ICON[kind],
              label: t(KIND_LABEL[kind]),
              onSelect: () => setPane({ mode: "new", kind }),
            }))}
          />
        )}
      </Show>
      <Show when={deleting()}>
        {(item) => (
          <ConfirmDialog
            title={t("vault.deleteTitle")}
            message={t("vault.deleteBody", { title: item().title })}
            confirmLabel={t("common.delete")}
            danger
            onCancel={() => setDeleting(null)}
            onConfirm={() => {
              const target = item();
              setDeleting(null);
              setPane({ mode: "none" });
              void app.guard(() => vault.remove(target.id));
            }}
          />
        )}
      </Show>
      <Show when={dialog() === "password"}>
        <ChangeVaultPasswordDialog
          onClose={() => {
            setDialog(null);
            props.onDismissWarning();
          }}
        />
      </Show>
      <Show when={dialog() === "generator"}>
        <GeneratorDialog onUse={() => setDialog(null)} onClose={() => setDialog(null)} />
      </Show>
      <Show when={dialog() === "csv"}>
        <CsvImportDialog onClose={() => setDialog(null)} />
      </Show>
    </div>
  );
}

/** The vault screen: set up, locked, or open. Locked shows nothing but the unlock form. */
export function VaultView(): JSX.Element {
  const { vault } = useApp();
  const [sameAsAccount, setSameAsAccount] = createSignal(false);
  return (
    <section class="vault-view" aria-label={t("vault.title")}>
      <Switch fallback={<Spinner />}>
        <Match when={vault.pendingRecoveryKey()}>
          {(key) => (
            <div class="vault-gate">
              <h1>{t("recovery.vaultTitle")}</h1>
              <Show when={sameAsAccount()}>
                <div class="warning-banner" role="alert">
                  <Icon name="alert" />
                  <span>{t("vault.samePasswordWarning")}</span>
                </div>
              </Show>
              <RecoveryKeyPanel recoveryKey={key()} subject="vault" onConfirm={() => vault.confirmRecoveryKey()} />
            </div>
          )}
        </Match>
        <Match when={vault.state() === "uninitialised"}>
          <VaultSetup onCreated={setSameAsAccount} />
        </Match>
        <Match when={vault.state() === "locked"}>
          <VaultUnlock />
        </Match>
        <Match when={vault.state() === "unlocked"}>
          <UnlockedVault sameAsAccount={sameAsAccount()} onDismissWarning={() => setSameAsAccount(false)} />
        </Match>
      </Switch>
    </section>
  );
}

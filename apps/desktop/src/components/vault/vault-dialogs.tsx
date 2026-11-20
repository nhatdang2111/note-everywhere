import { For, Show, createEffect, createSignal, type JSX } from "solid-js";
import type { GeneratePasswordArgs, VaultCsvFormat, VaultImportReport } from "../../api/contract.ts";
import { t, type MessageKey } from "../../i18n/index.ts";
import { MIN_PASSWORD_LENGTH } from "../../lib/password.ts";
import { createSubmit, invalid } from "../../lib/use-submit.ts";
import { useApp } from "../../state/app.tsx";
import { PasswordStrengthMeter } from "../auth/password-strength-meter.tsx";
import { Button } from "../common/button.tsx";
import { Dialog } from "../common/dialog.tsx";
import { Checkbox, PasswordField, Select } from "../common/form.tsx";
import { ErrorBanner } from "../common/states.tsx";

/** Asks for the vault password when the core wants it again before showing or copying a secret. */
export function ReauthDialog(): JSX.Element {
  const { vault } = useApp();
  const [password, setPassword] = createSignal("");
  return (
    <Show when={vault.reauth()}>
      {(request) => {
        // A new request (after a wrong password) starts with an empty box.
        createEffect(() => void (request().message, request().busy, setPassword("")));
        return (
          <Dialog title={t("vault.reauthTitle")} onClose={() => request().cancel()} dismissOnBackdrop={false}>
            <form
              class="form"
              onSubmit={(event) => {
                event.preventDefault();
                const value = password();
                setPassword("");
                request().submit(value);
              }}
            >
              <p>{t("vault.reauthIntro")}</p>
              <PasswordField label={t("vault.password")} value={password()} onInput={setPassword} autocomplete="off" autofocus />
              <ErrorBanner error={request().message} />
              <div class="form-actions">
                <Button onClick={() => request().cancel()}>{t("common.cancel")}</Button>
                <Button variant="primary" type="submit" busy={request().busy} disabled={!password()}>
                  {t("vault.reauthConfirm")}
                </Button>
              </div>
            </form>
          </Dialog>
        );
      }}
    </Show>
  );
}

export function ChangeVaultPasswordDialog(props: { onClose(): void }): JSX.Element {
  const { vault, toasts } = useApp();
  const [current, setCurrent] = createSignal("");
  const [next, setNext] = createSignal("");
  const [confirm, setConfirm] = createSignal("");
  const form = createSubmit(async () => {
    if (!current()) throw invalid(t("vault.enterPassword"));
    if (next().length < MIN_PASSWORD_LENGTH) throw invalid(t("auth.passwordTooShort", { min: MIN_PASSWORD_LENGTH }));
    if (next() !== confirm()) throw invalid(t("auth.passwordsDiffer"));
    await vault.changePassword(current(), next());
    toasts.push("success", t("vault.passwordChanged"));
    props.onClose();
  });
  return (
    <Dialog title={t("vault.changePassword")} onClose={props.onClose} dismissOnBackdrop={false}>
      <form class="form" onSubmit={form.submit}>
        <PasswordField label={t("vault.currentPassword")} value={current()} onInput={setCurrent} autocomplete="off" autofocus />
        <PasswordField label={t("auth.newPassword")} value={next()} onInput={setNext} autocomplete="new-password" />
        <PasswordStrengthMeter password={next()} />
        <PasswordField label={t("auth.confirmPassword")} value={confirm()} onInput={setConfirm} autocomplete="new-password" />
        <ErrorBanner error={form.error()} />
        <div class="form-actions">
          <Button onClick={props.onClose}>{t("common.cancel")}</Button>
          <Button variant="primary" type="submit" busy={form.busy()}>
            {t("vault.changePassword")}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}

/** Password generator. The result is only shown here and handed to the form; the core never stores it. */
export function GeneratorDialog(props: { onUse(value: string): void; onClose(): void }): JSX.Element {
  const { vault } = useApp();
  const [mode, setMode] = createSignal<GeneratePasswordArgs["mode"]>("characters");
  const [length, setLength] = createSignal(20);
  const [lower, setLower] = createSignal(true);
  const [upper, setUpper] = createSignal(true);
  const [digits, setDigits] = createSignal(true);
  const [symbols, setSymbols] = createSignal(false);
  const [unambiguous, setUnambiguous] = createSignal(true);
  const [value, setValue] = createSignal("");
  const [bits, setBits] = createSignal(0);

  const generate = createSubmit(async () => {
    const result = await vault.generate({
      mode: mode(),
      length: mode() === "passphrase" ? Math.min(length(), 12) : length(),
      lowercase: lower(),
      uppercase: upper(),
      digits: digits(),
      symbols: symbols(),
      excludeAmbiguous: unambiguous(),
    });
    setValue(result.value);
    setBits(result.entropyBits);
  });
  void generate.submit();

  return (
    <Dialog title={t("generator.title")} onClose={props.onClose}>
      <form class="form" onSubmit={generate.submit}>
        <Select
          label={t("generator.mode")}
          value={mode()}
          options={[
            { value: "characters", label: t("generator.characters") },
            { value: "passphrase", label: t("generator.passphrase") },
          ]}
          onChange={(next) => (setMode(next), setLength(next === "passphrase" ? 5 : 20))}
        />
        <div class="field">
          <label for="generator-length">{mode() === "passphrase" ? t("generator.words") : t("generator.length")}</label>
          <input
            id="generator-length"
            type="number"
            min={mode() === "passphrase" ? 3 : 8}
            max={mode() === "passphrase" ? 12 : 128}
            value={length()}
            onInput={(event) => setLength(Number(event.currentTarget.value) || 0)}
          />
        </div>
        <Show when={mode() === "characters"}>
          <fieldset class="checkbox-group">
            <legend>{t("generator.include")}</legend>
            <Checkbox label={t("generator.lowercase")} checked={lower()} onChange={setLower} />
            <Checkbox label={t("generator.uppercase")} checked={upper()} onChange={setUpper} />
            <Checkbox label={t("generator.digits")} checked={digits()} onChange={setDigits} />
            <Checkbox label={t("generator.symbols")} checked={symbols()} onChange={setSymbols} />
            <Checkbox label={t("generator.ambiguous")} checked={unambiguous()} onChange={setUnambiguous} />
          </fieldset>
        </Show>
        <ErrorBanner error={generate.error()} />
        <output class="generated" data-testid="generated-password" aria-live="polite">
          {value()}
        </output>
        <Show when={value()}>
          <p class="field-hint">{t("generator.entropy", { bits: bits() })}</p>
        </Show>
        <div class="form-actions">
          <Button icon="dice" type="submit" busy={generate.busy()}>
            {t("generator.again")}
          </Button>
          <Button variant="primary" disabled={!value()} onClick={() => props.onUse(value())}>
            {t("generator.use")}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}

const FORMATS: { value: VaultCsvFormat; label: MessageKey }[] = [
  { value: "generic", label: "csv.generic" },
  { value: "bitwarden", label: "csv.bitwarden" },
  { value: "one_password", label: "csv.onePassword" },
  { value: "last_pass", label: "csv.lastPass" },
];

export function CsvImportDialog(props: { onClose(): void }): JSX.Element {
  const { vault } = useApp();
  const [format, setFormat] = createSignal<VaultCsvFormat>("generic");
  const [report, setReport] = createSignal<VaultImportReport | null>(null);
  const [cancelled, setCancelled] = createSignal(false);
  const form = createSubmit(async () => {
    const result = await vault.importCsv(format());
    setCancelled(result === null);
    setReport(result);
  });
  return (
    <Dialog title={t("csv.title")} onClose={props.onClose} dismissOnBackdrop={false}>
      <Show
        when={report()}
        fallback={
          <form class="form" onSubmit={form.submit}>
            <p>{t("csv.intro")}</p>
            <Select label={t("csv.format")} value={format()} options={FORMATS.map((f) => ({ value: f.value, label: t(f.label) }))} onChange={setFormat} />
            <Show when={cancelled()}>
              <p class="field-hint">{t("csv.cancelled")}</p>
            </Show>
            <ErrorBanner error={form.error()} />
            <div class="form-actions">
              <Button onClick={props.onClose}>{t("common.cancel")}</Button>
              <Button variant="primary" type="submit" busy={form.busy()} icon="download">
                {t("csv.choose")}
              </Button>
            </div>
          </form>
        }
      >
        {(result) => (
          <div class="form" role="status">
            <p>{t("csv.imported", { count: result().imported })}</p>
            <Show when={result().skipped.length > 0}>
              <p>{t("csv.skipped", { count: result().skipped.length })}</p>
              <ul class="skip-list">
                <For each={result().skipped}>{(row) => <li>{t("csv.skippedRow", { row: row.row, reason: row.reason })}</li>}</For>
              </ul>
            </Show>
            <div class="form-actions">
              <Button variant="primary" onClick={props.onClose}>
                {t("common.done")}
              </Button>
            </div>
          </div>
        )}
      </Show>
    </Dialog>
  );
}

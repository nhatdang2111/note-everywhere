import { For, Show, createSignal, type JSX } from "solid-js";
import type { StructuredImportReport } from "../../api/contract.ts";
import { t } from "../../i18n/index.ts";
import { useApp } from "../../state/app.tsx";
import { Button } from "../common/button.tsx";
import { Dialog } from "../common/dialog.tsx";

/** The counts and everything passed over, as shown after a structured import. */
export function ImportReport(props: { report: StructuredImportReport }): JSX.Element {
  const rows = () =>
    [
      ["folders", props.report.folders],
      ["pages", props.report.pages],
      ["databases", props.report.databases],
      ["rows", props.report.rows],
      ["attachments", props.report.attachments],
      ["linksRewritten", props.report.linksRewritten],
      ["linksUnresolved", props.report.linksUnresolved],
      ["imagesMissing", props.report.imagesMissing],
    ] as const;
  return (
    <div class="import-report">
      <dl>
        <For each={rows()}>
          {([key, count]) => (
            <div class="report-row" data-warn={(key === "linksUnresolved" || key === "imagesMissing") && count > 0 ? "true" : undefined}>
              <dt>{t(`import.report.${key}`)}</dt>
              <dd>{count}</dd>
            </div>
          )}
        </For>
      </dl>
      <Show when={props.report.skipped.length > 0} fallback={<p class="field-hint">{t("import.report.nothingSkipped")}</p>}>
        <h3>{t("import.report.skipped", { count: props.report.skipped.length })}</h3>
        <ul class="skipped-list">
          <For each={props.report.skipped}>{(reason) => <li>{reason}</li>}</For>
        </ul>
      </Show>
    </div>
  );
}

/** Import another app's Markdown and CSV export folder, then show what happened. */
export function ImportStructuredDialog(props: { parentId: string | null; onClose(): void }): JSX.Element {
  const app = useApp();
  const parentId = props.parentId;
  const [busy, setBusy] = createSignal(false);
  const [report, setReport] = createSignal<StructuredImportReport | null>(null);
  const choose = async () => {
    setBusy(true);
    const result = await app.importStructured(parentId);
    setBusy(false);
    if (result) setReport(result);
  };
  return (
    <Dialog
      title={t("import.structuredTitle")}
      wide
      onClose={props.onClose}
      footer={
        <>
          <Show when={!report()}>
            <Button variant="primary" icon="download" busy={busy()} data-autofocus onClick={() => void choose()}>{t("import.chooseFolder")}</Button>
          </Show>
          <Button onClick={props.onClose}>{report() ? t("common.done") : t("common.cancel")}</Button>
        </>
      }
    >
      <Show when={report()} fallback={<><p>{t("import.structuredIntro")}</p><p class="field-hint">{t("import.structuredHint")}</p></>}>
        {(done) => <ImportReport report={done()} />}
      </Show>
    </Dialog>
  );
}

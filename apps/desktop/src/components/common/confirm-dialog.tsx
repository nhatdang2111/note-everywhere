import type { JSX } from "solid-js";
import { t } from "../../i18n/index.ts";
import { Button } from "./button.tsx";
import { Dialog } from "./dialog.tsx";

export interface ConfirmDialogProps {
  title: string;
  message: JSX.Element;
  confirmLabel: string;
  danger?: boolean;
  busy?: boolean;
  onConfirm(): void;
  onCancel(): void;
}

/** Yes/no question. Focus starts on Cancel so a stray Enter never destroys anything. */
export function ConfirmDialog(props: ConfirmDialogProps): JSX.Element {
  return (
    <Dialog
      title={props.title}
      onClose={props.onCancel}
      footer={
        <>
          <Button data-autofocus onClick={props.onCancel}>
            {t("common.cancel")}
          </Button>
          <Button variant={props.danger ? "danger" : "primary"} busy={props.busy} onClick={props.onConfirm}>
            {props.confirmLabel}
          </Button>
        </>
      }
    >
      <p>{props.message}</p>
    </Dialog>
  );
}

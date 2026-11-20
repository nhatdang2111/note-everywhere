import { createContext, useContext } from "solid-js";
import type { NodeId } from "../../api/contract.ts";
import type { DatabaseController } from "../../state/database-store.ts";

const DatabaseContext = createContext<DatabaseController>();

export const DatabaseProvider = DatabaseContext.Provider;

export function useDatabase(): DatabaseController {
  const controller = useContext(DatabaseContext);
  if (!controller) throw new Error("useDatabase must be used inside a database view");
  return controller;
}

/** Dialogs of the schema editor that menus all over the database view can ask for. */
export type SchemaDialog =
  | { kind: "add-property" }
  | { kind: "rename-property"; propertyId: string }
  | { kind: "change-type"; propertyId: string }
  | { kind: "delete-property"; propertyId: string }
  | { kind: "options"; propertyId: string }
  | { kind: "row-templates" };

export interface DatabaseActions {
  openSchemaDialog(request: SchemaDialog): void;
  /** Open a row as a page; the second argument opens it in a new tab. */
  openRow(rowId: NodeId, title: string, inNewTab?: boolean): void;
  trashRow(rowId: NodeId): Promise<void>;
}

const ActionsContext = createContext<DatabaseActions>();

export const DatabaseActionsProvider = ActionsContext.Provider;

export function useDatabaseActions(): DatabaseActions {
  const actions = useContext(ActionsContext);
  if (!actions) throw new Error("useDatabaseActions must be used inside a database view");
  return actions;
}

/** Like `useDatabaseActions`, for components that also work outside a database view. */
export const useDatabaseActionsOptional = (): DatabaseActions | undefined => useContext(ActionsContext);

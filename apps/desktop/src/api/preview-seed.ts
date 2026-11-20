import type { CoreApi } from "./contract.ts";

/**
 * Browser preview only: with `?demo` in the URL, start with a local workspace
 * holding a few synthetic pages so the shell has something to show.
 */
export async function seedPreview(api: CoreApi, params: URLSearchParams): Promise<void> {
  if (!params.has("demo")) return;
  await api.workspaceCreateLocal();
  const welcome = await api.treeCreate({ kind: "page", parentId: null, title: "Welcome" });
  const projects = await api.treeCreate({ kind: "folder", parentId: null, title: "Projects" });
  await api.treeCreate({ kind: "page", parentId: projects, title: "Garden plan" });
  const trip = await api.treeCreate({ kind: "page", parentId: projects, title: "Trip to the coast" });
  await api.treeCreate({ kind: "page", parentId: trip, title: "Packing list" });
  await api.treeCreate({ kind: "page", parentId: null, title: "Ghi chú họp" });
  await api.treeSetFavorite({ id: welcome, favorite: true });
}

import { describe, expect, it } from "vitest";
import { fireEvent, screen, waitFor, within } from "@solidjs/testing-library";
import { MEMORY_SETUP_CODE } from "../../api/memory.ts";
import { GOOD_PASSWORD, SERVER_URL, renderApp, seedLocal, seedSignedIn, type RenderedApp } from "../../test/app-harness.tsx";

const openSettings = async (ui: RenderedApp) => {
  await ui.user.click(await screen.findByRole("button", { name: "Settings" }));
  return screen.findByRole("region", { name: "Settings" });
};

const typeIn = async (ui: RenderedApp, label: string, value: string, scope: HTMLElement) => {
  const input = within(scope).getByLabelText(label);
  await ui.user.clear(input);
  await ui.user.type(input, value);
};

describe("settings: appearance and vault", () => {
  it("changes the theme and the vault idle lock time through the core", async () => {
    const ui = await renderApp({ setup: async (memory) => void (await seedLocal(memory)) });
    const settings = await openSettings(ui);
    fireEvent.change(within(settings).getByLabelText("Theme"), { target: { value: "dark" } });
    await waitFor(() => expect(document.documentElement.dataset.theme).toBe("dark"));
    expect((await ui.memory.api.settingsGet()).theme).toBe("dark");

    fireEvent.change(within(settings).getByLabelText("Lock the vault after"), { target: { value: "900" } });
    await waitFor(async () => expect((await ui.memory.api.settingsGet()).vaultIdleLockSeconds).toBe(900));
  });

  it("restores the previous value when the core refuses a change", async () => {
    const ui = await renderApp({ setup: async (memory) => void (await seedLocal(memory)) });
    ui.memory.api.settingsSet = () => Promise.reject({ code: "readonly", message: "Settings are locked." });
    const settings = await openSettings(ui);
    fireEvent.change(within(settings).getByLabelText("Theme"), { target: { value: "dark" } });
    expect((await screen.findByRole("alert")).textContent).toContain("Settings are locked.");
    await waitFor(() => expect((within(settings).getByLabelText("Theme") as HTMLSelectElement).value).toBe("system"));
  });

  it("switches the interface to Vietnamese and back", async () => {
    const ui = await renderApp({ setup: async (memory) => void (await seedLocal(memory)) });
    const settings = await openSettings(ui);
    const appearance = () => within(settings.querySelector('[aria-labelledby="appearance-title"]') as HTMLElement);
    fireEvent.change(appearance().getByLabelText("Language"), { target: { value: "vi" } });
    expect(await screen.findByRole("heading", { name: "Cài đặt" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Thùng rác" })).toBeTruthy();
    expect(localStorage.getItem("ne.locale")).toBe("vi");
    fireEvent.change(appearance().getByLabelText("Ngôn ngữ"), { target: { value: "en" } });
    expect(await screen.findByRole("heading", { name: "Settings" })).toBeTruthy();
  });
});

describe("settings: local workspace", () => {
  it("offers to turn on sync and continues through sign-up to the recovery key", async () => {
    const ui = await renderApp({ setup: async (memory) => void (await seedLocal(memory)) });
    const settings = await openSettings(ui);
    expect(within(settings).getByText("This workspace lives only on this device.")).toBeTruthy();
    await ui.user.click(within(settings).getByRole("button", { name: "Sync with my server" }));
    const dialog = await screen.findByRole("dialog", { name: "Sync with my server" });
    const form = within(dialog).getByRole("form", { name: "Create account" });
    await ui.user.type(within(form).getByLabelText("Server address"), SERVER_URL);
    await ui.user.type(within(form).getByLabelText("Setup code"), MEMORY_SETUP_CODE);
    await ui.user.type(within(form).getByLabelText("Username"), "ada");
    await ui.user.type(within(form).getByLabelText("Password"), GOOD_PASSWORD);
    await ui.user.type(within(form).getByLabelText("Confirm password"), GOOD_PASSWORD);
    await ui.user.click(within(form).getByRole("button", { name: "Create account" }));
    expect(await screen.findByTestId("recovery-key")).toBeTruthy();
    expect(screen.queryByRole("navigation", { name: "Workspace" })).toBeNull();
  });
});

describe("settings: signed-in account", () => {
  const signedIn = async () => {
    const ui = await renderApp({ setup: async (memory) => void (await seedSignedIn(memory)) });
    const settings = await openSettings(ui);
    return { ui, settings };
  };

  it("shows who is signed in and where", async () => {
    const { settings } = await signedIn();
    expect(within(settings).getByText("ada")).toBeTruthy();
    expect(within(settings).getByText(SERVER_URL)).toBeTruthy();
  });

  it("validates and changes the password", async () => {
    const { ui, settings } = await signedIn();
    const form = within(settings).getByRole("form", { name: "Change password" });
    await typeIn(ui, "Current password", "not mine at all", form);
    await typeIn(ui, "New password", "short", form);
    await typeIn(ui, "Confirm password", "short", form);
    await ui.user.click(within(form).getByRole("button", { name: "Change password" }));
    expect((await within(form).findByRole("alert")).textContent).toContain("at least 10 characters");

    await typeIn(ui, "New password", "my next long password", form);
    await typeIn(ui, "Confirm password", "my next long password", form);
    await ui.user.click(within(form).getByRole("button", { name: "Change password" }));
    expect((await within(form).findByRole("alert")).textContent).toContain("not right");

    await typeIn(ui, "Current password", GOOD_PASSWORD, form);
    await ui.user.click(within(form).getByRole("button", { name: "Change password" }));
    expect(await screen.findByText("Password changed.")).toBeTruthy();
    expect((within(form).getByLabelText("Current password") as HTMLInputElement).value).toBe("");
  });

  it("lists devices and revokes another one after confirming", async () => {
    const { ui, settings } = await signedIn();
    const list = await within(settings).findByRole("list", {}, { timeout: 2000 }).catch(() => settings.querySelector(".device-list") as HTMLElement);
    const rows = () => [...settings.querySelectorAll<HTMLElement>(".device-row")];
    await waitFor(() => expect(rows().length).toBe(3));
    void list;
    const current = rows().find((row) => row.textContent?.includes("This device")) as HTMLElement;
    expect(within(current).queryByRole("button", { name: "Revoke" })).toBeNull();
    const other = rows().find((row) => row.textContent?.includes("Travel laptop")) as HTMLElement;
    await ui.user.click(within(other).getByRole("button", { name: "Revoke" }));
    const dialog = await screen.findByRole("dialog", { name: "Revoke this device?" });
    await ui.user.click(within(dialog).getByRole("button", { name: "Revoke" }));
    expect(await screen.findByText("“Travel laptop” was signed out.")).toBeTruthy();
    await waitFor(() => expect(rows().length).toBe(2));
  });

  it("signs out after confirming and asks for the password again", async () => {
    const { ui, settings } = await signedIn();
    await ui.user.click(within(settings).getByRole("button", { name: "Sign out" }));
    const dialog = await screen.findByRole("dialog", { name: "Sign out?" });
    await ui.user.click(within(dialog).getByRole("button", { name: "Sign out" }));
    expect(await screen.findByText("Password needed again")).toBeTruthy();
    expect((await ui.memory.api.sessionStatus()).state).toBe("needs_sign_in");
  });
});

describe("import and export actions", () => {
  it("imports Markdown files and reports the count", async () => {
    const files = [new File(["# One\n\nbody"], "One.md"), new File(["two"], "Two.md")];
    const ui = await renderApp({ pickFiles: async () => files, setup: async (memory) => void (await seedLocal(memory)) });
    const settings = await openSettings(ui);
    await ui.user.click(within(settings).getByRole("button", { name: "Import Markdown…" }));
    expect(await screen.findByText("Imported 2 page(s) from Markdown.")).toBeTruthy();
    await waitFor(async () => expect((await ui.memory.api.treeSnapshot()).map((n) => n.title).sort()).toEqual(["One", "Two"]));
    expect(within(screen.getByRole("tree")).getByText("One")).toBeTruthy();
  });

  it("says nothing when the picker is cancelled", async () => {
    const ui = await renderApp({ pickFiles: async () => [], setup: async (memory) => void (await seedLocal(memory)) });
    const settings = await openSettings(ui);
    await ui.user.click(within(settings).getByRole("button", { name: "Import Markdown…" }));
    await new Promise((resolve) => setTimeout(resolve, 30));
    expect(screen.queryByText(/Imported/)).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("exports everything and one subtree from the menus", async () => {
    const saved: string[] = [];
    const ui = await renderApp({ saveFile: (name) => saved.push(name), setup: async (memory) => void (await seedLocal(memory, ["Alpha", "Beta"])) });
    await ui.user.click(await screen.findByRole("button", { name: "Workspace menu" }));
    await ui.user.click(await screen.findByRole("menuitem", { name: "Export everything as Markdown…" }));
    expect(await screen.findByText("Exported 2 page(s) as Markdown.")).toBeTruthy();

    fireEvent.contextMenu(within(screen.getByRole("tree")).getByText("Beta"));
    await ui.user.click(await screen.findByRole("menuitem", { name: "Export as Markdown…" }));
    await waitFor(() => expect(saved).toEqual(["workspace.md", "Beta.md"]));
  });

  it("shows the core's message when export fails", async () => {
    const ui = await renderApp({ setup: async (memory) => void (await seedLocal(memory, ["Alpha"])) });
    ui.memory.api.exportMarkdown = () => Promise.reject({ code: "no_space", message: "The destination folder is full." });
    await ui.user.click(await screen.findByRole("button", { name: "Workspace menu" }));
    await ui.user.click(await screen.findByRole("menuitem", { name: "Export everything as Markdown…" }));
    expect((await screen.findByRole("alert")).textContent).toContain("The destination folder is full.");
  });
});

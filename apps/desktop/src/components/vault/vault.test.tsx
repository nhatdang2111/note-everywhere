import { afterEach, describe, expect, it, vi } from "vitest";
import { fireEvent, screen, waitFor, within } from "@solidjs/testing-library";
import type { MemoryCore } from "../../api/memory.ts";
import { GOOD_PASSWORD, renderApp, seedLocal, seedSignedIn, tick, type RenderedApp } from "../../test/app-harness.tsx";

const SECRET = "hunter2-synthetic-secret";
const NOTES = "synthetic note body";

async function openVault(ui: RenderedApp) {
  await ui.user.click(await screen.findByRole("button", { name: "Vault" }));
  return screen.findByRole("region", { name: "Vault" });
}

/** A vault that already exists, optionally holding one login with a secret. */
async function prepared(memory: MemoryCore, { items = true, locked = false } = {}) {
  await seedLocal(memory);
  await memory.api.vaultCreate({ password: GOOD_PASSWORD });
  if (items) {
    await memory.api.vaultSave({ item: { kind: "login", title: "Mail", username: "ada@example.test", password: SECRET, notes: NOTES, urls: ["https://mail.example.test"], tags: ["personal"] } });
    await memory.api.vaultSave({ item: { kind: "api_key", title: "Deploy token", value: "tok_synthetic", tags: ["work"] } });
  }
  if (locked) await memory.api.vaultLock();
}

const passwordRow = (view: HTMLElement) => within(view).getByText("Password", { selector: ".secret-label" }).closest(".secret-row") as HTMLElement;

const typeIn = async (ui: RenderedApp, label: string, value: string, scope: HTMLElement | Document = document) => {
  const input = within(scope as HTMLElement).getByLabelText(label);
  await ui.user.clear(input);
  await ui.user.type(input, value);
};

afterEach(() => vi.restoreAllMocks());

describe("vault setup", () => {
  it("creates the vault, then holds on the recovery key until it is confirmed", async () => {
    const ui = await renderApp({ setup: async (m) => void (await seedLocal(m)) });
    const view = await openVault(ui);
    expect(await within(view).findByText("Set up your vault")).toBeTruthy();

    await typeIn(ui, "Vault password", "short", view);
    await typeIn(ui, "Confirm password", "short", view);
    await ui.user.click(within(view).getByRole("button", { name: "Create vault" }));
    expect((await within(view).findByRole("alert")).textContent).toContain("at least 10 characters");

    await typeIn(ui, "Vault password", GOOD_PASSWORD, view);
    await typeIn(ui, "Confirm password", "something else entirely", view);
    await ui.user.click(within(view).getByRole("button", { name: "Create vault" }));
    expect((await within(view).findByRole("alert")).textContent).toContain("do not match");
    expect(await ui.memory.api.vaultState()).toBe("uninitialised");

    await typeIn(ui, "Confirm password", GOOD_PASSWORD, view);
    await ui.user.click(within(view).getByRole("button", { name: "Create vault" }));
    const key = (await within(view).findByTestId("recovery-key")).textContent ?? "";
    expect(key).toMatch(/^[A-Z2-9]{4}(-[A-Z2-9]{4}){5}$/);
    expect(within(view).queryByText("The vault is empty")).toBeNull();

    const next = within(view).getByRole("button", { name: "Continue" }) as HTMLButtonElement;
    expect(next.disabled).toBe(true);
    await ui.user.click(within(view).getByRole("checkbox", { name: /saved this recovery key/i }));
    await ui.user.click(next);
    expect(await within(view).findByText("The vault is empty")).toBeTruthy();
    expect(within(view).queryByText(key)).toBeNull();
  });

  it("warns when the vault password equals the account password and lets the person change it", async () => {
    const ui = await renderApp({ setup: async (m) => void (await seedSignedIn(m)) });
    const view = await openVault(ui);
    await typeIn(ui, "Vault password", GOOD_PASSWORD, view);
    await typeIn(ui, "Confirm password", GOOD_PASSWORD, view);
    await ui.user.click(within(view).getByRole("button", { name: "Create vault" }));
    expect((await within(view).findByRole("alert")).textContent).toContain("same as your account password");
    await ui.user.click(within(view).getByRole("checkbox", { name: /saved this recovery key/i }));
    await ui.user.click(within(view).getByRole("button", { name: "Continue" }));

    expect((await within(view).findByRole("alert")).textContent).toContain("same as your account password");
    await ui.user.click(within(view).getAllByRole("button", { name: "Change vault password" })[0] as HTMLElement);
    const dialog = await screen.findByRole("dialog", { name: "Change vault password" });
    await typeIn(ui, "Current password", GOOD_PASSWORD, dialog);
    await typeIn(ui, "New password", "a different long one", dialog);
    await typeIn(ui, "Confirm password", "a different long one", dialog);
    await ui.user.click(within(dialog).getByRole("button", { name: "Change vault password" }));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Change vault password" })).toBeNull());
    expect(await screen.findByText("Vault password changed.")).toBeTruthy();
    expect(within(view).queryByText(/same as your account password/)).toBeNull();
  });
});

describe("locking", () => {
  it("shows only the unlock form while locked and rejects a wrong password", async () => {
    const ui = await renderApp({ setup: async (m) => void (await prepared(m, { locked: true })) });
    const view = await openVault(ui);
    expect(await within(view).findByText("The vault is locked")).toBeTruthy();
    expect(document.body.textContent).not.toContain("Mail");
    await typeIn(ui, "Vault password", "wrong password!", view);
    await ui.user.click(within(view).getByRole("button", { name: "Unlock" }));
    expect((await within(view).findByRole("alert")).textContent).toContain("not right");
    expect(document.body.textContent).not.toContain("Mail");

    await typeIn(ui, "Vault password", GOOD_PASSWORD, view);
    await ui.user.click(within(view).getByRole("button", { name: "Unlock" }));
    expect(await within(view).findByText("Mail")).toBeTruthy();
    expect(within(view).getByText("Deploy token")).toBeTruthy();
  });

  it("locks on request and hides everything, including a revealed secret", async () => {
    const ui = await renderApp({ setup: async (m) => void (await prepared(m)) });
    const view = await openVault(ui);
    await ui.user.click(await within(view).findByText("Mail"));
    await within(view).findByText("Password", { selector: ".secret-label" });
    await ui.user.click(within(passwordRow(view)).getByRole("button", { name: "Reveal" }));
    expect(within(view).getByTestId("secret-password").textContent).toBe(SECRET);

    await ui.user.click(within(view).getByRole("button", { name: "Lock now" }));
    expect(await within(view).findByText("The vault is locked")).toBeTruthy();
    expect(document.body.textContent).not.toContain(SECRET);
    expect(document.body.textContent).not.toContain("Mail");
  });

  it("reacts to an automatic lock from the core by clearing the screen", async () => {
    const ui = await renderApp({ setup: async (m) => void (await prepared(m)) });
    const view = await openVault(ui);
    await ui.user.click(await within(view).findByText("Mail"));
    await within(view).findByText("Password", { selector: ".secret-label" });
    await ui.user.click(within(passwordRow(view)).getByRole("button", { name: "Reveal" }));
    expect(document.body.textContent).toContain(SECRET);
    ui.memory.control.autoLock();
    expect(await within(view).findByText("The vault is locked")).toBeTruthy();
    expect(document.body.textContent).not.toContain(SECRET);
    expect(document.body.textContent).not.toContain("ada@example.test");
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("closes the re-authentication prompt when the vault locks underneath it", async () => {
    const ui = await renderApp({ reauthGraceMs: 0, setup: async (m) => void (await prepared(m)) });
    const view = await openVault(ui);
    await ui.user.click(await within(view).findByText("Mail"));
    await within(view).findByText("Password", { selector: ".secret-label" });
    await ui.user.click(within(passwordRow(view)).getByRole("button", { name: "Reveal" }));
    await screen.findByRole("dialog", { name: "Confirm it is you" });
    ui.memory.control.autoLock();
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Confirm it is you" })).toBeNull());
  });

  it("recovers a forgotten password with the recovery key", async () => {
    let recoveryKey = "";
    const ui = await renderApp({
      setup: async (memory) => {
        await seedLocal(memory);
        recoveryKey = (await memory.api.vaultCreate({ password: GOOD_PASSWORD })).recoveryKey;
        await memory.api.vaultLock();
      },
    });
    const view = await openVault(ui);
    await ui.user.click(await within(view).findByRole("button", { name: "Forgot the password?" }));
    await typeIn(ui, "Recovery key", "WRONG-KEY", view);
    await typeIn(ui, "New password", "brand new vault pass", view);
    await ui.user.click(within(view).getByRole("button", { name: "Recover vault" }));
    expect((await within(view).findByRole("alert")).textContent).toContain("does not match");
    await typeIn(ui, "Recovery key", recoveryKey, view);
    await ui.user.click(within(view).getByRole("button", { name: "Recover vault" }));
    expect(await within(view).findByText("The vault is empty")).toBeTruthy();
  });
});

describe("items", () => {
  it("lists items without any secret, and shows details only on selection", async () => {
    const ui = await renderApp({ setup: async (m) => void (await prepared(m)) });
    const view = await openVault(ui);
    await within(view).findByText("Mail");
    expect(document.body.textContent).not.toContain(SECRET);
    expect(document.body.textContent).not.toContain("tok_synthetic");
    await ui.user.click(within(view).getByText("Mail"));
    expect(await within(view).findByText("https://mail.example.test")).toBeTruthy();
    expect(within(view).getAllByLabelText("Hidden").length).toBeGreaterThan(0);
    expect(document.body.textContent).not.toContain(SECRET);
  });

  it("creates each of the three kinds of item", async () => {
    const ui = await renderApp({ setup: async (m) => void (await prepared(m, { items: false })) });
    const view = await openVault(ui);
    const add = async (kind: string) => {
      await ui.user.click(await within(view).findByRole("button", { name: "New item" }));
      await ui.user.click(await screen.findByRole("menuitem", { name: kind }));
    };

    await add("login");
    await typeIn(ui, "Title", "Bank", view);
    await typeIn(ui, "Username", "ada", view);
    await typeIn(ui, "Password", "bank-secret-1", view);
    await ui.user.click(within(view).getByRole("button", { name: "Save" }));
    expect(await within(view).findByRole("heading", { name: "Bank" })).toBeTruthy();

    await add("secure note");
    await typeIn(ui, "Title", "Safe code", view);
    await typeIn(ui, "Notes", "left, right, left", view);
    await ui.user.click(within(view).getByRole("button", { name: "Save" }));
    expect(await within(view).findByRole("heading", { name: "Safe code" })).toBeTruthy();

    await add("API key");
    await typeIn(ui, "Title", "Maps key", view);
    await typeIn(ui, "API key", "key-123", view);
    await ui.user.click(within(view).getByRole("button", { name: "Save" }));
    expect(await within(view).findByRole("heading", { name: "Maps key" })).toBeTruthy();

    const items = await ui.memory.api.vaultList({});
    expect(items.map((item) => item.title).sort()).toEqual(["Bank", "Maps key", "Safe code"]);
  });

  it("requires a title and keeps stored secrets when they are left blank on edit", async () => {
    const ui = await renderApp({ setup: async (m) => void (await prepared(m)) });
    const view = await openVault(ui);
    await ui.user.click(await within(view).findByText("Mail"));
    await ui.user.click(within(view).getByRole("button", { name: "Edit" }));
    await typeIn(ui, "Title", "  ", view);
    await ui.user.click(within(view).getByRole("button", { name: "Save" }));
    expect((await within(view).findByRole("alert")).textContent).toContain("Give the item a title.");
    await typeIn(ui, "Title", "Mail renamed", view);
    await ui.user.click(within(view).getByRole("button", { name: "Save" }));
    expect(await within(view).findByRole("heading", { name: "Mail renamed" })).toBeTruthy();
    const [mail] = await ui.memory.api.vaultList({ query: "renamed" });
    expect(await ui.memory.api.vaultReveal({ id: mail?.id as string, field: "password" })).toBe(SECRET);
  });

  it("searches through the core and filters by tag", async () => {
    const ui = await renderApp({ setup: async (m) => void (await prepared(m)) });
    const view = await openVault(ui);
    await within(view).findByText("Mail");
    await ui.user.type(within(view).getByLabelText("Search the vault"), "deploy");
    await waitFor(() => expect(within(view).queryByText("Mail")).toBeNull());
    expect(within(view).getByText("Deploy token")).toBeTruthy();
    await ui.user.clear(within(view).getByLabelText("Search the vault"));
    await waitFor(() => expect(within(view).getByText("Mail")).toBeTruthy());

    await ui.user.click(within(view).getByRole("button", { name: "work" }));
    expect(within(view).queryByText("Mail")).toBeNull();
    expect(within(view).getByText("Deploy token")).toBeTruthy();
    await ui.user.click(within(view).getByRole("button", { name: "work" }));
    expect(within(view).getByText("Mail")).toBeTruthy();
  });

  it("says when a search finds nothing", async () => {
    const ui = await renderApp({ setup: async (m) => void (await prepared(m)) });
    const view = await openVault(ui);
    await within(view).findByText("Mail");
    await ui.user.type(within(view).getByLabelText("Search the vault"), "nothing like this");
    expect(await within(view).findByText("Nothing matches")).toBeTruthy();
  });

  it("deletes an item after confirmation", async () => {
    const ui = await renderApp({ setup: async (m) => void (await prepared(m)) });
    const view = await openVault(ui);
    await ui.user.click(await within(view).findByText("Mail"));
    await ui.user.click(within(view).getByRole("button", { name: "Delete" }));
    const dialog = await screen.findByRole("dialog", { name: "Delete this item?" });
    await ui.user.click(within(dialog).getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(within(view).queryByText("Mail")).toBeNull());
    expect((await ui.memory.api.vaultList({})).map((item) => item.title)).toEqual(["Deploy token"]);
  });
});

describe("revealing and copying secrets", () => {
  it("reveals on demand, hides again, and never logs a secret", async () => {
    const spies = (["log", "info", "warn", "error", "debug"] as const).map((method) => vi.spyOn(console, method).mockImplementation(() => undefined));
    const ui = await renderApp({ setup: async (m) => void (await prepared(m)) });
    const view = await openVault(ui);
    await ui.user.click(await within(view).findByText("Mail"));
    const row = (await within(view).findByText("Password", { selector: ".secret-label" })).closest(".secret-row") as HTMLElement;
    expect(row.textContent).not.toContain(SECRET);
    await ui.user.click(within(row).getByRole("button", { name: "Reveal" }));
    expect(await within(row).findByTestId("secret-password")).toBeTruthy();
    expect(row.textContent).toContain(SECRET);
    await ui.user.click(within(row).getByRole("button", { name: "Hide" }));
    expect(row.textContent).not.toContain(SECRET);
    for (const spy of spies) for (const call of spy.mock.calls) expect(JSON.stringify(call)).not.toContain(SECRET);
  });

  it("asks for the vault password when the core requires re-authentication", async () => {
    const ui = await renderApp({ reauthGraceMs: 0, setup: async (m) => void (await prepared(m)) });
    const view = await openVault(ui);
    await ui.user.click(await within(view).findByText("Mail"));
    const row = (await within(view).findByText("Password", { selector: ".secret-label" })).closest(".secret-row") as HTMLElement;
    await ui.user.click(within(row).getByRole("button", { name: "Reveal" }));

    const dialog = await screen.findByRole("dialog", { name: "Confirm it is you" });
    await typeIn(ui, "Vault password", "wrong password!", dialog);
    await ui.user.click(within(dialog).getByRole("button", { name: "Continue" }));
    expect((await within(dialog).findByRole("alert")).textContent).toContain("not right");
    expect(row.textContent).not.toContain(SECRET);

    await typeIn(ui, "Vault password", GOOD_PASSWORD, dialog);
    await ui.user.click(within(dialog).getByRole("button", { name: "Continue" }));
    await waitFor(() => expect(row.textContent).toContain(SECRET));
    expect(screen.queryByRole("dialog", { name: "Confirm it is you" })).toBeNull();
  });

  it("leaves the secret hidden and shows no error when the prompt is cancelled", async () => {
    const ui = await renderApp({ reauthGraceMs: 0, setup: async (m) => void (await prepared(m)) });
    const view = await openVault(ui);
    await ui.user.click(await within(view).findByText("Mail"));
    const row = (await within(view).findByText("Password", { selector: ".secret-label" })).closest(".secret-row") as HTMLElement;
    await ui.user.click(within(row).getByRole("button", { name: "Reveal" }));
    const dialog = await screen.findByRole("dialog", { name: "Confirm it is you" });
    await ui.user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Confirm it is you" })).toBeNull());
    expect(row.textContent).not.toContain(SECRET);
    expect(within(row).queryByRole("alert")).toBeNull();
  });

  it("copies through the core and shows a countdown until the clipboard is cleared", async () => {
    const ui = await renderApp({ clipboardSeconds: 3, setup: async (m) => void (await prepared(m)) });
    const view = await openVault(ui);
    await ui.user.click(await within(view).findByText("Mail"));
    const row = (await within(view).findByText("Password", { selector: ".secret-label" })).closest(".secret-row") as HTMLElement;
    await ui.user.click(within(row).getByRole("button", { name: "Copy" }));
    expect((await within(row).findByTestId("clipboard-countdown")).textContent).toBe("Copied. The clipboard is cleared in 3 s.");
    expect(ui.memory.control.clipboard()).toBe(SECRET);
    expect(row.textContent).not.toContain(SECRET);
    await waitFor(() => expect(within(row).getByTestId("clipboard-countdown").textContent).toContain("2 s"), { timeout: 2500 });
    await waitFor(() => expect(within(row).queryByTestId("clipboard-countdown")).toBeNull(), { timeout: 3500 });
  }, 10_000);
});

describe("generator and CSV import", () => {
  it("generates a password to the chosen options and hands it to the item form", async () => {
    const ui = await renderApp({ setup: async (m) => void (await prepared(m, { items: false })) });
    const view = await openVault(ui);
    await ui.user.click(await within(view).findByRole("button", { name: "New item" }));
    await ui.user.click(await screen.findByRole("menuitem", { name: "login" }));
    const form = within(view).getByRole("form", { name: "New item" });
    await ui.user.click(within(form).getByRole("button", { name: "Generate password" }));

    const dialog = await screen.findByRole("dialog", { name: "Password generator" });
    await waitFor(() => expect(within(dialog).getByTestId("generated-password").textContent?.length).toBeGreaterThan(8));
    const first = within(dialog).getByTestId("generated-password").textContent;

    await ui.user.click(within(dialog).getByRole("checkbox", { name: "Lowercase letters" }));
    await ui.user.click(within(dialog).getByRole("checkbox", { name: "Uppercase letters" }));
    await ui.user.click(within(dialog).getByRole("button", { name: "Generate again" }));
    await waitFor(() => expect(within(dialog).getByTestId("generated-password").textContent).toMatch(/^\d{20}$/));
    expect(within(dialog).getByTestId("generated-password").textContent).not.toBe(first);

    const generated = within(dialog).getByTestId("generated-password").textContent as string;
    await ui.user.click(within(dialog).getByRole("button", { name: "Use this password" }));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Password generator" })).toBeNull());
    await typeIn(ui, "Title", "Generated", view);
    await ui.user.click(within(view).getByRole("button", { name: "Save" }));
    const [saved] = await ui.memory.api.vaultList({ query: "Generated" });
    expect(await ui.memory.api.vaultReveal({ id: saved?.id as string, field: "password" })).toBe(generated);
  });

  it("offers a passphrase and says how strong the result is", async () => {
    const ui = await renderApp({ setup: async (m) => void (await prepared(m, { items: false })) });
    const view = await openVault(ui);
    await ui.user.click(await within(view).findByRole("button", { name: "Generate password" }));
    const dialog = await screen.findByRole("dialog", { name: "Password generator" });
    fireEvent.change(within(dialog).getByLabelText("Kind"), { target: { value: "passphrase" } });
    await ui.user.click(within(dialog).getByRole("button", { name: "Generate again" }));
    await waitFor(() => expect(within(dialog).getByTestId("generated-password").textContent).toMatch(/^[a-z]+(-[a-z]+){4}$/));
    expect(within(dialog).getByText(/bits of strength/)).toBeTruthy();
  });

  it("imports a CSV file and reports what was skipped", async () => {
    const csv = "title,username,password,url,notes\nBank,ada,pw1,https://bank.test,\n,nobody,pw2,,\nShop,ada,pw3,,\n";
    const ui = await renderApp({ pickFiles: async () => [new File([csv], "export.csv")], setup: async (m) => void (await prepared(m, { items: false })) });
    const view = await openVault(ui);
    await ui.user.click(await within(view).findByRole("button", { name: "Import from CSV" }));
    const dialog = await screen.findByRole("dialog", { name: "Import from CSV" });
    await ui.user.click(within(dialog).getByRole("button", { name: "Choose file…" }));
    expect(await within(dialog).findByText("Items imported: 2")).toBeTruthy();
    expect(within(dialog).getByText("Rows skipped: 1")).toBeTruthy();
    expect(within(dialog).getByText("Row 3: Missing title")).toBeTruthy();
    await ui.user.click(within(dialog).getByRole("button", { name: "Done" }));
    expect(await within(view).findByText("Bank")).toBeTruthy();
    expect(within(view).getByText("Shop")).toBeTruthy();
  });

  it("tells the person when no file was chosen", async () => {
    const ui = await renderApp({ pickFiles: async () => [], setup: async (m) => void (await prepared(m, { items: false })) });
    const view = await openVault(ui);
    await ui.user.click(await within(view).findByRole("button", { name: "Import from CSV" }));
    const dialog = await screen.findByRole("dialog", { name: "Import from CSV" });
    await ui.user.click(within(dialog).getByRole("button", { name: "Choose file…" }));
    expect(await within(dialog).findByText("No file was chosen.")).toBeTruthy();
  });
});

describe("changing the vault password", () => {
  it("rejects a wrong current password and accepts the right one", async () => {
    const ui = await renderApp({ setup: async (m) => void (await prepared(m, { items: false })) });
    const view = await openVault(ui);
    await ui.user.click(await within(view).findByRole("button", { name: "Change vault password" }));
    const dialog = await screen.findByRole("dialog", { name: "Change vault password" });
    await typeIn(ui, "Current password", "not the password", dialog);
    await typeIn(ui, "New password", "yet another long one", dialog);
    await typeIn(ui, "Confirm password", "yet another long one", dialog);
    await ui.user.click(within(dialog).getByRole("button", { name: "Change vault password" }));
    expect((await within(dialog).findByRole("alert")).textContent).toContain("not right");
    await typeIn(ui, "Current password", GOOD_PASSWORD, dialog);
    await ui.user.click(within(dialog).getByRole("button", { name: "Change vault password" }));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Change vault password" })).toBeNull());
    await ui.memory.api.vaultLock();
    await ui.memory.api.vaultUnlock({ password: "yet another long one" });
    await tick();
  });
});

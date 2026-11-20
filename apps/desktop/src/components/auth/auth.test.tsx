import { describe, expect, it } from "vitest";
import { screen, waitFor, within } from "@solidjs/testing-library";
import { MEMORY_SETUP_CODE } from "../../api/memory.ts";
import { GOOD_PASSWORD, SERVER_URL, renderApp, seedSignedIn } from "../../test/app-harness.tsx";

async function chooseServer(ui: Awaited<ReturnType<typeof renderApp>>) {
  await ui.user.click(await screen.findByRole("button", { name: /sync with my server/i }));
}

async function fillSignUp(ui: Awaited<ReturnType<typeof renderApp>>, overrides: Partial<Record<"url" | "code" | "name" | "pw" | "confirm", string>> = {}) {
  const form = await screen.findByRole("form", { name: "Create account" });
  const { user } = ui;
  await user.type(within(form).getByLabelText("Server address"), overrides.url ?? SERVER_URL);
  await user.type(within(form).getByLabelText("Setup code"), overrides.code ?? MEMORY_SETUP_CODE);
  await user.type(within(form).getByLabelText("Username"), overrides.name ?? "ada");
  await user.type(within(form).getByLabelText("Password"), overrides.pw ?? GOOD_PASSWORD);
  await user.type(within(form).getByLabelText("Confirm password"), overrides.confirm ?? overrides.pw ?? GOOD_PASSWORD);
  return form;
}

describe("first run", () => {
  it("offers a local workspace and a server, and opens the workspace for the local choice", async () => {
    const ui = await renderApp();
    expect(await screen.findByText("Where should your notes live?")).toBeTruthy();
    expect(screen.getByRole("button", { name: /sync with my server/i })).toBeTruthy();
    await ui.user.click(screen.getByRole("button", { name: /use on this device only/i }));
    expect(await screen.findByRole("navigation", { name: "Workspace" })).toBeTruthy();
    expect((await ui.memory.api.sessionStatus()).state).toBe("local");
  });

  it("shows the core's message when creating the workspace fails", async () => {
    const ui = await renderApp({
      setup: async (memory) => {
        memory.api.workspaceCreateLocal = () => Promise.reject({ code: "disk_full", message: "There is no space left on this disk." });
      },
    });
    await ui.user.click(await screen.findByRole("button", { name: /use on this device only/i }));
    expect((await screen.findByRole("alert")).textContent).toContain("There is no space left on this disk.");
  });
});

describe("sign-up and the recovery key", () => {
  it("checks the password before calling the core", async () => {
    const ui = await renderApp();
    await chooseServer(ui);
    const form = await fillSignUp(ui, { pw: "short", confirm: "short" });
    await ui.user.click(within(form).getByRole("button", { name: "Create account" }));
    expect((await within(form).findByRole("alert")).textContent).toContain("at least 10 characters");

    await ui.user.clear(within(form).getByLabelText("Password"));
    await ui.user.type(within(form).getByLabelText("Password"), GOOD_PASSWORD);
    await ui.user.click(within(form).getByRole("button", { name: "Create account" }));
    expect((await within(form).findByRole("alert")).textContent).toContain("do not match");
    expect((await ui.memory.api.sessionStatus()).state).toBe("empty");
  });

  it("shows the strength hint while typing", async () => {
    const ui = await renderApp();
    await chooseServer(ui);
    const form = await screen.findByRole("form", { name: "Create account" });
    expect(within(form).getByText(/at least 10 characters\. longer is better/i)).toBeTruthy();
    await ui.user.type(within(form).getByLabelText("Password"), "abc");
    expect(within(form).getByText(/too short/i)).toBeTruthy();
    await ui.user.type(within(form).getByLabelText("Password"), "Zq7!kd9#Lm2@Pw");
    expect(within(form).getByText("Strong")).toBeTruthy();
  });

  it("shows the server's error message", async () => {
    const ui = await renderApp();
    await chooseServer(ui);
    const form = await fillSignUp(ui, { code: "wrong-code" });
    await ui.user.click(within(form).getByRole("button", { name: "Create account" }));
    expect((await within(form).findByRole("alert")).textContent).toBe("That setup code was not accepted by the server.");
  });

  it("will not continue past the recovery key until it is confirmed saved", async () => {
    const ui = await renderApp();
    await chooseServer(ui);
    const form = await fillSignUp(ui);
    await ui.user.click(within(form).getByRole("button", { name: "Create account" }));

    const key = (await screen.findByTestId("recovery-key")).textContent ?? "";
    expect(key).toMatch(/^[A-Z2-9]{4}(-[A-Z2-9]{4}){5}$/);
    expect(screen.queryByRole("navigation", { name: "Workspace" })).toBeNull();

    const next = screen.getByRole("button", { name: "Continue" }) as HTMLButtonElement;
    expect(next.disabled).toBe(true);
    await ui.user.click(next);
    expect(screen.getByTestId("recovery-key")).toBeTruthy();

    await ui.user.click(screen.getByRole("checkbox", { name: /i have saved this recovery key/i }));
    expect(next.disabled).toBe(false);
    await ui.user.click(next);

    expect(await screen.findByRole("navigation", { name: "Workspace" })).toBeTruthy();
    expect(screen.queryByText(key)).toBeNull();
    expect((await ui.memory.api.sessionStatus()).state).toBe("signed_in");
  });
});

describe("signing in again", () => {
  it("asks for the password again when the session was rejected and then opens the workspace", async () => {
    const ui = await renderApp({ setup: async (memory) => void (await seedSignedIn(memory, ["Kept page"])) });
    expect(await screen.findByRole("navigation", { name: "Workspace" })).toBeTruthy();
    ui.memory.control.expireSession();

    expect(await screen.findByText("Password needed again")).toBeTruthy();
    expect((screen.getByLabelText("Username") as HTMLInputElement).value).toBe("ada");
    expect((screen.getByLabelText("Server address") as HTMLInputElement).value).toBe(SERVER_URL);

    await ui.user.type(screen.getByLabelText("Password"), "not the password");
    await ui.user.click(screen.getByRole("button", { name: "Sign in" }));
    expect((await screen.findByRole("alert")).textContent).toContain("not right");

    await ui.user.clear(screen.getByLabelText("Password"));
    await ui.user.type(screen.getByLabelText("Password"), GOOD_PASSWORD);
    await ui.user.click(screen.getByRole("button", { name: "Sign in" }));
    expect(await screen.findByRole("navigation", { name: "Workspace" })).toBeTruthy();
    expect(await screen.findByText("Kept page")).toBeTruthy();
  });

  it("recovers with the recovery key", async () => {
    let recoveryKey = "";
    const ui = await renderApp({
      setup: async (memory) => {
        const result = await memory.api.signUp({ serverUrl: SERVER_URL, setupCode: MEMORY_SETUP_CODE, username: "ada", password: GOOD_PASSWORD, deviceLabel: "Laptop" });
        recoveryKey = result.recoveryKey;
        await memory.api.signOut();
      },
    });
    await ui.user.click(await screen.findByRole("button", { name: "Use my recovery key instead" }));
    const form = await screen.findByRole("form", { name: "Recover" });
    await ui.user.type(within(form).getByLabelText("Recovery key"), recoveryKey);
    await ui.user.type(within(form).getByLabelText("New password"), "a brand new password");
    await ui.user.type(within(form).getByLabelText("Confirm password"), "a brand new password");
    await ui.user.click(within(form).getByRole("button", { name: "Recover and sign in" }));
    expect(await screen.findByRole("navigation", { name: "Workspace" })).toBeTruthy();
    await waitFor(async () => expect((await ui.memory.api.sessionStatus()).state).toBe("signed_in"));
  });
});

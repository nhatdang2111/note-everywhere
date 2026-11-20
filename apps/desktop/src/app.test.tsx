import { describe, expect, it } from "vitest";
import { render, screen } from "@solidjs/testing-library";
import { Bootstrap } from "./app.tsx";
import { loadCore } from "./api/index.ts";
import { createMemoryCore } from "./api/memory.ts";

describe("app bootstrap", () => {
  it("mounts in preview mode with a banner and the first-run screen", async () => {
    render(() => <Bootstrap load={loadCore} />);
    expect(await screen.findByText("Where should your notes live?")).toBeTruthy();
    expect(screen.getByRole("note").textContent).toContain("Browser preview");
  });

  it("shows why it cannot start when no core is available", async () => {
    render(() => <Bootstrap load={() => Promise.reject(new Error("Open the installed app instead."))} />);
    expect((await screen.findByRole("alert")).textContent).toContain("Open the installed app instead.");
    expect(screen.getByText("Could not start")).toBeTruthy();
  });

  it("offers a retry when the session cannot be read", async () => {
    const memory = createMemoryCore();
    let attempts = 0;
    const real = memory.api.sessionStatus;
    memory.api.sessionStatus = async () => {
      attempts += 1;
      if (attempts === 1) throw { code: "db_locked", message: "The workspace database is busy." };
      return real();
    };
    render(() => <Bootstrap load={async () => ({ api: memory.api, events: memory.events, preview: false })} />);
    expect((await screen.findByRole("alert")).textContent).toContain("The workspace database is busy.");
    screen.getByRole("button", { name: "Try again" }).click();
    expect(await screen.findByText("Where should your notes live?")).toBeTruthy();
  });
});

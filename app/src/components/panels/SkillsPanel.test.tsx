// Component-level tests for SkillsPanel (PRD-13). The Tauri invoke
// bridge is stubbed via `src/test/setup.ts`; assertions run on the
// rendered DOM.

import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import SkillsPanel from "./SkillsPanel";
import type { Skill } from "../../ipc/skills";

const SKILL_A: Skill = {
  id: "skill-a",
  name: "Alpha",
  description: "alpha description",
  version: "1.0.0",
  author: "test",
  created: "2026-04-18",
  tags: ["t1"],
  applicable_contexts: ["ai-chat"],
  triggers: ["alpha"],
  parameters: [],
  body: "body of alpha",
};

const SKILL_B: Skill = {
  ...SKILL_A,
  id: "skill-b",
  name: "Beta",
  description: "beta description",
  body: "body of beta",
};

describe("SkillsPanel", () => {
  it("renders the empty state when the registry has no skills", async () => {
    globalThis.__setInvokeHandlers({ skills_list: async () => [] });
    render(<SkillsPanel />);

    expect(await screen.findByText(/No skills in/)).toBeInTheDocument();
    expect(screen.getByText("Skills (0)")).toBeInTheDocument();
  });

  it("lists skills and auto-selects the first one", async () => {
    globalThis.__setInvokeHandlers({
      skills_list: async () => [SKILL_A, SKILL_B],
    });
    render(<SkillsPanel />);

    // Auto-select first → detail pane shows Alpha's body.
    expect(
      await screen.findByRole("heading", { name: "Alpha" }),
    ).toBeInTheDocument();
    expect(screen.getByText("body of alpha")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Beta/ })).toBeInTheDocument();
    expect(screen.getByText("Skills (2)")).toBeInTheDocument();
  });

  it("switches detail pane when a different skill is clicked", async () => {
    globalThis.__setInvokeHandlers({
      skills_list: async () => [SKILL_A, SKILL_B],
    });
    render(<SkillsPanel />);

    await screen.findByRole("heading", { name: "Alpha" });

    const betaRow = screen.getByRole("button", { name: /Beta/ });
    await userEvent.click(betaRow);

    expect(screen.getByRole("heading", { name: "Beta" })).toBeInTheDocument();
    expect(screen.getByText("body of beta")).toBeInTheDocument();
    expect(betaRow).toHaveAttribute("aria-pressed", "true");
  });

  it("reload button calls skills_reload then re-lists", async () => {
    let listCalls = 0;
    let reloadCalls = 0;
    globalThis.__setInvokeHandlers({
      skills_list: async () => {
        listCalls += 1;
        return listCalls === 1 ? [SKILL_A] : [SKILL_A, SKILL_B];
      },
      skills_reload: async () => {
        reloadCalls += 1;
        return { loaded: 2 };
      },
    });
    render(<SkillsPanel />);

    await screen.findByRole("heading", { name: "Alpha" });
    expect(screen.queryByRole("button", { name: /Beta/ })).not.toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "Reload" }));

    expect(await screen.findByRole("button", { name: /Beta/ })).toBeInTheDocument();
    expect(reloadCalls).toBe(1);
    expect(listCalls).toBe(2);
  });

  it("surfaces an error when the IPC call rejects", async () => {
    globalThis.__setInvokeHandlers({
      skills_list: async () => {
        throw new Error("bus down");
      },
    });
    render(<SkillsPanel />);

    const alert = await screen.findByRole("alert");
    expect(within(alert).getByText(/bus down/)).toBeInTheDocument();
  });
});

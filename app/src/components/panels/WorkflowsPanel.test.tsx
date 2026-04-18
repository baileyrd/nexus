// Component-level tests for WorkflowsPanel (PRD-16).

import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import WorkflowsPanel from "./WorkflowsPanel";
import type { Workflow } from "../../ipc/workflow";

const GREET: Workflow = {
  workflow: { name: "Greet", description: "noop smoke", version: "1.0.0" },
  trigger: { type: "manual" },
  steps: [{ type: "noop", name: "hello" }],
};

const DAILY: Workflow = {
  workflow: { name: "Daily", description: "cron smoke" },
  trigger: { type: "cron", schedule: "0 9 * * *" },
  steps: [{ type: "file_create", name: "journal" }],
};

describe("WorkflowsPanel", () => {
  it("renders the empty state when no workflows are declared", async () => {
    globalThis.__setInvokeHandlers({ workflow_list: async () => [] });
    render(<WorkflowsPanel />);

    expect(await screen.findByText(/No workflows in/)).toBeInTheDocument();
    expect(screen.getByText("Workflows (0)")).toBeInTheDocument();
  });

  it("lists workflows and auto-selects the first", async () => {
    globalThis.__setInvokeHandlers({
      workflow_list: async () => [GREET, DAILY],
    });
    render(<WorkflowsPanel />);

    expect(
      await screen.findByRole("heading", { name: "Greet" }),
    ).toBeInTheDocument();
    // Trigger row rendered for the selected workflow.
    expect(screen.getAllByText(/manual/).length).toBeGreaterThan(0);
    expect(screen.getByRole("button", { name: /Daily/ })).toBeInTheDocument();
  });

  it("switches detail pane when a different workflow is clicked", async () => {
    globalThis.__setInvokeHandlers({
      workflow_list: async () => [GREET, DAILY],
    });
    render(<WorkflowsPanel />);

    await screen.findByRole("heading", { name: "Greet" });

    const dailyRow = screen.getByRole("button", { name: /Daily/ });
    await userEvent.click(dailyRow);

    expect(screen.getByRole("heading", { name: "Daily" })).toBeInTheDocument();
    // Cron schedule surfaces in the trigger row.
    expect(screen.getByText(/0 9 \* \* \*/)).toBeInTheDocument();
    expect(dailyRow).toHaveAttribute("aria-pressed", "true");
  });

  it("reload button calls workflow_reload then re-lists", async () => {
    let listCalls = 0;
    let reloadCalls = 0;
    globalThis.__setInvokeHandlers({
      workflow_list: async () => {
        listCalls += 1;
        return listCalls === 1 ? [GREET] : [GREET, DAILY];
      },
      workflow_reload: async () => {
        reloadCalls += 1;
        return { loaded: 2 };
      },
    });
    render(<WorkflowsPanel />);

    await screen.findByRole("heading", { name: "Greet" });
    expect(screen.queryByRole("button", { name: /Daily/ })).not.toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "Reload" }));

    expect(
      await screen.findByRole("button", { name: /Daily/ }),
    ).toBeInTheDocument();
    expect(reloadCalls).toBe(1);
    expect(listCalls).toBe(2);
  });

  it("surfaces an error when the IPC call rejects", async () => {
    globalThis.__setInvokeHandlers({
      workflow_list: async () => {
        throw new Error("workflow plugin offline");
      },
    });
    render(<WorkflowsPanel />);

    const alert = await screen.findByRole("alert");
    expect(within(alert).getByText(/workflow plugin offline/)).toBeInTheDocument();
  });
});

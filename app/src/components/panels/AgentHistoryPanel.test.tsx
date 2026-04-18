// Component-level tests for AgentHistoryPanel (PRD-15).

import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import AgentHistoryPanel from "./AgentHistoryPanel";
import type {
  AgentHistoryEntry,
  AgentHistoryRecord,
} from "../../ipc/agent";

const ENTRY_A: AgentHistoryEntry = {
  plan_id: "plan-a",
  goal: "write a haiku",
  created_at: "ts-1713420000",
  success: true,
  steps: 2,
  bytes: 420,
};

const ENTRY_B: AgentHistoryEntry = {
  plan_id: "plan-b",
  goal: "refactor module",
  created_at: "ts-1713430000", // newer
  success: false,
  steps: 3,
  bytes: 512,
};

const RECORD_A: AgentHistoryRecord = {
  plan_id: "plan-a",
  goal: "write a haiku",
  created_at: "ts-1713420000",
  plan: {
    id: "plan-a",
    goal: "write a haiku",
    steps: [
      { id: "s1", description: "draft lines", tool_call: null },
      { id: "s2", description: "polish rhythm", tool_call: null },
    ],
  },
  observation: {
    plan_id: "plan-a",
    success: true,
    steps: [
      { step_id: "s1", response: null, status: "ok" },
      { step_id: "s2", response: null, status: "ok" },
    ],
  },
};

const RECORD_B: AgentHistoryRecord = {
  ...RECORD_A,
  plan_id: "plan-b",
  goal: "refactor module",
  created_at: "ts-1713430000",
  plan: {
    id: "plan-b",
    goal: "refactor module",
    steps: [
      { id: "s1", description: "read file", tool_call: null },
      { id: "s2", description: "edit", tool_call: null },
      { id: "s3", description: "save", tool_call: null },
    ],
  },
  observation: {
    plan_id: "plan-b",
    success: false,
    steps: [
      { step_id: "s1", response: null, status: "ok" },
      { step_id: "s2", response: null, status: "failed" },
      { step_id: "s3", response: null, status: "skipped" },
    ],
  },
};

describe("AgentHistoryPanel", () => {
  it("renders the empty state when no history is persisted", async () => {
    globalThis.__setInvokeHandlers({ agent_history_list: async () => [] });
    render(<AgentHistoryPanel />);

    expect(await screen.findByText(/No runs yet/)).toBeInTheDocument();
    expect(screen.getByText(/0 total/)).toBeInTheDocument();
  });

  it("lists runs newest-first and auto-loads the first record", async () => {
    globalThis.__setInvokeHandlers({
      agent_history_list: async () => [ENTRY_A, ENTRY_B],
      agent_history_get: async (args) => {
        // Newest-first sort means plan-b (ts-1713430000) is selected.
        expect(args?.planId).toBe("plan-b");
        return RECORD_B;
      },
    });
    render(<AgentHistoryPanel />);

    // Heading comes from `goal` of the auto-selected record.
    expect(
      await screen.findByRole("heading", { name: "refactor module" }),
    ).toBeInTheDocument();
    expect(screen.getByText(/2 total · 1 ok · 1 failed/)).toBeInTheDocument();
  });

  it("loads a different record when a list item is clicked", async () => {
    const getCalls: string[] = [];
    globalThis.__setInvokeHandlers({
      agent_history_list: async () => [ENTRY_A, ENTRY_B],
      agent_history_get: async (args) => {
        const id = args?.planId as string;
        getCalls.push(id);
        return id === "plan-a" ? RECORD_A : RECORD_B;
      },
    });
    render(<AgentHistoryPanel />);

    await screen.findByRole("heading", { name: "refactor module" });

    const olderRow = screen.getByRole("button", { name: /write a haiku/ });
    await userEvent.click(olderRow);

    expect(
      await screen.findByRole("heading", { name: "write a haiku" }),
    ).toBeInTheDocument();
    expect(getCalls).toContain("plan-a");
  });

  it("delete button calls agent_history_delete then re-lists", async () => {
    let listCalls = 0;
    let deleted: string | null = null;
    globalThis.__setInvokeHandlers({
      agent_history_list: async () => {
        listCalls += 1;
        return listCalls === 1 ? [ENTRY_A] : [];
      },
      agent_history_get: async () => RECORD_A,
      agent_history_delete: async (args) => {
        deleted = (args?.planId as string) ?? null;
      },
    });
    render(<AgentHistoryPanel />);

    await screen.findByRole("heading", { name: "write a haiku" });

    await userEvent.click(screen.getByRole("button", { name: "Delete" }));

    expect(deleted).toBe("plan-a");
    expect(await screen.findByText(/No runs yet/)).toBeInTheDocument();
    expect(listCalls).toBe(2);
  });
});

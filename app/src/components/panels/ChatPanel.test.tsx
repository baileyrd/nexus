// Component-level tests for ChatPanel. Scoped tightly to the four
// surfaces the last session shipped: archetype selector, stepwise
// approval (PendingPlanCard), session picker, and the agent-preview
// flow that turns a user message into a pending plan. The panel's
// streaming-chat + RAG paths are out of scope here — they're covered
// by the `com.nexus.ai` IPC contract tests.

import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";

import { ChatPanel } from "./ChatPanel";
import type { AiConfigSnapshot } from "../../ipc/ai";
import type { AgentPlan, StepResult } from "../../ipc/agent";

const CONFIG: AiConfigSnapshot = {
  ai: { provider: "anthropic", model: null, base_url: null, has_api_key: true },
  embedding: null,
};

const PLAN: AgentPlan = {
  id: "plan-test-1",
  goal: "smoke",
  steps: [
    {
      id: "s1",
      description: "write a scratch file",
      tool_call: {
        target_plugin_id: "com.nexus.storage",
        command_id: "write_file",
        args: { path: "x.txt", bytes: [] },
      },
    },
    {
      id: "s2",
      description: "informational",
      tool_call: null,
    },
  ],
};

function baseHandlers(extra: Record<string, (args?: Record<string, unknown>) => unknown> = {}) {
  return {
    ai_config: async () => CONFIG,
    ai_session_load: async () => null,
    ai_session_list: async () => [],
    ai_session_save: async () => undefined,
    ...extra,
  };
}

describe("ChatPanel", () => {
  beforeEach(() => {
    globalThis.__setInvokeHandlers(baseHandlers());
  });

  it("renders the toolbar and defaults to the general archetype", async () => {
    render(<ChatPanel />);

    expect(
      await screen.findByRole("combobox", { name: /Agent archetype/i }),
    ).toHaveValue("general");
    // Archetype selector is disabled until Agent mode turns on.
    expect(screen.getByRole("combobox", { name: /Agent archetype/i })).toBeDisabled();
    expect(screen.getByRole("button", { name: /^Agent$/ })).toBeInTheDocument();
  });

  it("passes the selected archetype to agent_plan in preview mode", async () => {
    const planCalls: Array<{ goal?: string; archetype?: string }> = [];
    globalThis.__setInvokeHandlers(
      baseHandlers({
        agent_plan: async (args) => {
          planCalls.push({
            goal: args?.goal as string,
            archetype: args?.archetype as string,
          });
          return PLAN;
        },
      }),
    );
    render(<ChatPanel />);

    await screen.findByText(/AI · anthropic/);

    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: /^Agent$/ }));
    const previewBtn = await screen.findByRole("button", { name: /^Preview$/ });
    await user.click(previewBtn);
    await waitFor(() => {
      expect(previewBtn).toHaveAttribute("aria-pressed", "true");
    });
    await user.selectOptions(
      screen.getByRole("combobox", { name: /Agent archetype/i }),
      "coder",
    );

    const textarea = screen.getByPlaceholderText(/message/i);
    await user.type(textarea, "refactor foo");
    await user.keyboard("{Enter}");

    await waitFor(() => {
      expect(planCalls).toHaveLength(1);
    });
    expect(planCalls[0]).toEqual({ goal: "refactor foo", archetype: "coder" });

    expect(
      await screen.findByText(/Plan awaiting approval · 2 steps/),
    ).toBeInTheDocument();
  });

  it("stepwise-approval button calls agent_execute_step with the current index", async () => {
    const stepCalls: Array<{ index: number; plan_id: string }> = [];
    const stepResult: StepResult = {
      step_id: "s1",
      response: { ok: true },
      status: "ok",
    };
    globalThis.__setInvokeHandlers(
      baseHandlers({
        agent_plan: async () => PLAN,
        agent_execute_step: async (args) => {
          stepCalls.push({
            index: args?.index as number,
            plan_id: (args?.plan as AgentPlan).id,
          });
          return stepResult;
        },
      }),
    );
    render(<ChatPanel />);

    await screen.findByText(/AI · anthropic/);

    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: /^Agent$/ }));
    const previewBtn = await screen.findByRole("button", { name: /^Preview$/ });
    await user.click(previewBtn);

    const textarea = screen.getByPlaceholderText(/message/i);
    await user.type(textarea, "go");
    await user.keyboard("{Enter}");

    const stepBtn = await screen.findByRole("button", { name: /Step \(one\)/ });
    await user.click(stepBtn);

    await waitFor(() => {
      expect(stepCalls).toHaveLength(1);
    });
    expect(stepCalls[0]).toEqual({ index: 0, plan_id: "plan-test-1" });

    expect(await screen.findByText(/Plan step 2 of 2/)).toBeInTheDocument();
  });

  it("session picker loads a different session when changed", async () => {
    const loadCalls: string[] = [];
    globalThis.__setInvokeHandlers(
      baseHandlers({
        ai_session_load: async (args) => {
          loadCalls.push((args?.id as string) ?? "<legacy>");
          return null;
        },
        ai_session_list: async () => [
          { id: "default", title: "default", updated_at: null, bytes: 0 },
          { id: "scratch", title: "scratch", updated_at: null, bytes: 0 },
        ],
      }),
    );
    render(<ChatPanel />);

    const picker = await screen.findByRole("combobox", { name: /Chat session/i });
    await waitFor(() => {
      expect(picker.querySelectorAll("option")).toHaveLength(2);
    });

    loadCalls.length = 0;
    await userEvent.setup().selectOptions(picker, "scratch");

    await waitFor(() => {
      expect(loadCalls).toContain("scratch");
    });
  });
});

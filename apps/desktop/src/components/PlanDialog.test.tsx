import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { HOME, assignTicket, mockBackend } from "../test/fixtures";
import { PlanDialog } from "./PlanDialog";

describe("PlanDialog", () => {
  it("shows affected files with existing and proposed values", () => {
    mockBackend({});
    render(<PlanDialog ticket={assignTicket} home={HOME} onClose={() => {}} />);
    const dialog = screen.getByRole("dialog", { name: /assign profile 'work'/i });
    expect(within(dialog).getByText("~/work/api/.git/config")).toBeInTheDocument();
    const emailRow = within(dialog).getByRole("row", { name: /user\.email/ });
    expect(within(emailRow).getByText("wrong@example.com")).toBeInTheDocument();
    expect(within(emailRow).getByText("ada@acme.example")).toBeInTheDocument();
    const nameRow = within(dialog).getByRole("row", { name: /user\.name/ });
    expect(within(nameRow).getByText("Not set")).toBeInTheDocument();
    expect(within(dialog).getByText(/GIT_AUTHOR_EMAIL is set/)).toBeInTheDocument();
  });

  it("applies the plan by token and reports the outcome", async () => {
    const backend = mockBackend({
      apply_plan: {
        operation: { id: "20261015T110332Z-85e23c", summary: "x", status: "applied", created_at: 0, action: assignTicket.plan.action, changes: [], backups: [] },
        effective: { name: "Ada Lovelace", email: "ada@acme.example" },
        notes: [],
      },
    });
    const onClose = vi.fn();
    render(<PlanDialog ticket={assignTicket} home={HOME} onClose={onClose} />);
    await userEvent.click(screen.getByRole("button", { name: "Apply changes" }));
    expect(backend).toHaveBeenCalledWith("apply_plan", { token: "tok-123" });
    expect(await screen.findByText(/ada@acme\.example/, { selector: "p" })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Done" }));
    expect(onClose).toHaveBeenCalledWith(true);
  });

  it("cancelling discards the pending plan", async () => {
    const backend = mockBackend({ discard_plan: null });
    const onClose = vi.fn();
    render(<PlanDialog ticket={assignTicket} home={HOME} onClose={onClose} />);
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(backend).toHaveBeenCalledWith("discard_plan", { token: "tok-123" });
    expect(onClose).toHaveBeenCalledWith(false);
  });

  it("blocks applying when the plan has conflicts", () => {
    mockBackend({});
    const ticket = {
      ...assignTicket,
      plan: { ...assignTicket.plan, conflicts: [{ code: "existing_include", message: "An includeIf for ~/work already exists" }] },
    };
    render(<PlanDialog ticket={ticket} home={HOME} onClose={() => {}} />);
    expect(screen.getByRole("button", { name: "Apply changes" })).toBeDisabled();
    expect(screen.getByText(/already exists/)).toBeInTheDocument();
  });

  it("explains a no-op plan instead of offering to apply", () => {
    mockBackend({});
    const ticket = { ...assignTicket, plan: { ...assignTicket.plan, changes: [], warnings: [] } };
    render(<PlanDialog ticket={ticket} home={HOME} onClose={() => {}} />);
    expect(screen.getByText(/already up to date/i)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Apply changes" })).not.toBeInTheDocument();
  });

  it("shows backend errors from applying without closing", async () => {
    mockBackend({
      apply_plan: () => {
        throw {
          code: "concurrent_modification",
          message: "configuration changed since the plan was made",
          fields: [],
          details: ["user.email in /x: expected [], found [\"other\"]"],
        };
      },
    });
    render(<PlanDialog ticket={assignTicket} home={HOME} onClose={() => {}} />);
    await userEvent.click(screen.getByRole("button", { name: "Apply changes" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(/changed since the plan was made/);
  });
});

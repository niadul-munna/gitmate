import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import App from "./App";
import { HOME, appInfo, handmadeRule, mockBackend, personalProfile, repoReport, workProfile, workRule } from "./test/fixtures";

const backend = (extra: Record<string, unknown> = {}) =>
  mockBackend({
    app_info: appInfo,
    list_profiles: [workProfile, personalProfile],
    list_rules: [workRule, handmadeRule],
    list_history: [],
    inspect_repository: repoReport,
    run_doctor: { ...appInfo, profile_count: 2, rules: [workRule], repository: null, findings: [] },
    ...extra,
  });

describe("App", () => {
  it("invites the user to choose a repository when none is selected", async () => {
    backend();
    render(<App />);
    expect(await screen.findByRole("heading", { name: "Overview" })).toBeInTheDocument();
    expect(screen.getByText(/choose a repository/i)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /choose repository/i })).toBeInTheDocument();
  });

  it("shows the effective identity, how Git decided, and the mismatch", async () => {
    localStorage.setItem("gitmate.repository", `${HOME}/work/api`);
    backend();
    render(<App />);
    expect(await screen.findByText("wrong@example.com", { selector: "p *" })).toBeInTheDocument();
    const trace = screen.getByRole("list", { name: /how git decided/i });
    const items = within(trace).getAllByRole("listitem");
    expect(items).toHaveLength(3);
    expect(items[2]).toHaveTextContent(/in effect/i);
    expect(items[0]).toHaveTextContent(/overridden/i);
    expect(screen.getAllByText(/expects profile 'work'/).length).toBeGreaterThan(0);
  });

  it("navigates between screens with the keyboard", async () => {
    backend();
    render(<App />);
    await screen.findByRole("heading", { name: "Overview" });
    const nav = screen.getByRole("navigation", { name: "Main" });
    const profiles = within(nav).getByRole("button", { name: "Profiles" });
    profiles.focus();
    await userEvent.keyboard("{Enter}");
    expect(await screen.findByRole("heading", { name: "Profiles" })).toBeInTheDocument();
    expect(profiles).toHaveAttribute("aria-current", "page");
  });

  it("summarises diagnostics with correct grammar", async () => {
    backend({
      run_doctor: { ...appInfo, profile_count: 2, rules: [workRule], repository: null, findings: repoReport.findings },
    });
    render(<App />);
    await userEvent.click(await screen.findByRole("button", { name: "Diagnostics" }));
    expect(await screen.findByRole("status")).toHaveTextContent(/^1 problem needs attention\./);
  });

  it("lists managed and hand-written rules separately", async () => {
    backend();
    render(<App />);
    await userEvent.click(await screen.findByRole("button", { name: "Rules" }));
    expect(await screen.findByRole("heading", { name: "Rules" })).toBeInTheDocument();
    expect(screen.getByText("~/work/")).toBeInTheDocument();
    expect(screen.getByText(/not managed by GitMate/i)).toBeInTheDocument();
  });

  it("asks before deleting a profile", async () => {
    const mocked = backend({
      get_profile: { profile: personalProfile, rules: [], repositories: [], ssh_key: null },
      delete_profile: null,
    });
    render(<App />);
    await userEvent.click(await screen.findByRole("button", { name: "Profiles" }));
    await userEvent.click(await screen.findByRole("button", { name: /personal/ }));
    await userEvent.click(await screen.findByRole("button", { name: "Delete profile" }));
    const confirm = screen.getByRole("dialog", { name: /delete profile 'personal'/i });
    expect(mocked).not.toHaveBeenCalledWith("delete_profile", expect.anything());
    await userEvent.click(within(confirm).getByRole("button", { name: "Delete" }));
    expect(mocked).toHaveBeenCalledWith("delete_profile", { id: "personal" });
  });
});

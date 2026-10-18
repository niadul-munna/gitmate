import { open } from "@tauri-apps/plugin-dialog";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { mockBackend, workProfile } from "../test/fixtures";
import { ProfileForm } from "./ProfileForm";

describe("ProfileForm", () => {
  it("labels every field and requires id, name and email", async () => {
    const backend = mockBackend({});
    render(<ProfileForm onSaved={() => {}} onCancel={() => {}} />);
    await userEvent.click(screen.getByRole("button", { name: "Create profile" }));
    expect(screen.getByLabelText("Profile id")).toHaveAccessibleDescription(/required/i);
    expect(screen.getByLabelText("Name")).toHaveAttribute("aria-invalid", "true");
    expect(screen.getByLabelText("Email")).toHaveAttribute("aria-invalid", "true");
    expect(backend).not.toHaveBeenCalledWith("create_profile", expect.anything());
  });

  it("creates a profile and shows backend field errors next to the field", async () => {
    const backend = mockBackend({
      create_profile: () => {
        throw {
          code: "invalid_profile",
          message: "invalid profile",
          fields: [{ field: "ssh_key", message: "/home/ada/.ssh/missing does not exist" }],
          details: [],
        };
      },
    });
    render(<ProfileForm onSaved={() => {}} onCancel={() => {}} />);
    await userEvent.type(screen.getByLabelText("Profile id"), "work");
    await userEvent.type(screen.getByLabelText("Name"), "Ada Lovelace");
    await userEvent.type(screen.getByLabelText("Email"), "ada@acme.example");
    await userEvent.type(screen.getByLabelText(/SSH private key/), "~/.ssh/missing");
    await userEvent.click(screen.getByRole("button", { name: "Create profile" }));
    expect(backend).toHaveBeenCalledWith("create_profile", {
      profile: { id: "work", name: "Ada Lovelace", email: "ada@acme.example", label: null, ssh_key: "~/.ssh/missing" },
    });
    const key = screen.getByLabelText(/SSH private key/);
    expect(key).toHaveAttribute("aria-invalid", "true");
    expect(key).toHaveAccessibleDescription(/does not exist/);
  });

  it("edits an existing profile without changing its id", async () => {
    const backend = mockBackend({ update_profile: { profile: workProfile, warnings: [] } });
    const onSaved = vi.fn();
    render(<ProfileForm profile={workProfile} onSaved={onSaved} onCancel={() => {}} />);
    expect(screen.queryByLabelText("Profile id")).not.toBeInTheDocument();
    const email = screen.getByLabelText("Email");
    await userEvent.clear(email);
    await userEvent.type(email, "new@acme.example");
    await userEvent.click(screen.getByRole("button", { name: "Save changes" }));
    expect(backend).toHaveBeenCalledWith("update_profile", {
      id: "work",
      profile: { name: "Ada Lovelace", email: "new@acme.example", label: "Acme (work GitHub)", ssh_key: "~/.ssh/id_acme" },
    });
    expect(onSaved).toHaveBeenCalled();
  });

  const sshKeys = {
    dir: "/home/ada/.ssh",
    keys: [
      { path: "~/.ssh/id_acme", used_by: ["work"] },
      { path: "~/.ssh/id_ed25519", used_by: [] },
    ],
  };

  it("fills the key field with one click on a key found in ~/.ssh", async () => {
    mockBackend({ list_ssh_keys: sshKeys });
    render(<ProfileForm onSaved={() => {}} onCancel={() => {}} />);
    await userEvent.click(await screen.findByRole("button", { name: /^id_ed25519/ }));
    expect(screen.getByLabelText(/SSH private key/)).toHaveValue("~/.ssh/id_ed25519");
  });

  it("says which other profile already uses a key", async () => {
    mockBackend({ list_ssh_keys: sshKeys });
    render(<ProfileForm onSaved={() => {}} onCancel={() => {}} />);
    expect(await screen.findByRole("button", { name: /^id_acme/ })).toHaveTextContent(/used by work/);
    expect(screen.getByRole("button", { name: /^id_ed25519/ })).not.toHaveTextContent(/used by/);
  });

  it("does not call the profile being edited another user of its own key", async () => {
    mockBackend({ list_ssh_keys: sshKeys });
    render(<ProfileForm profile={workProfile} onSaved={() => {}} onCancel={() => {}} />);
    expect(await screen.findByRole("button", { name: /^id_acme/ })).not.toHaveTextContent(/used by/);
  });

  it("opens the file picker inside ~/.ssh", async () => {
    mockBackend({ list_ssh_keys: sshKeys });
    render(<ProfileForm onSaved={() => {}} onCancel={() => {}} />);
    await screen.findByRole("button", { name: /^id_acme/ });
    await userEvent.click(screen.getByRole("button", { name: "Choose…" }));
    expect(vi.mocked(open)).toHaveBeenCalledWith(expect.objectContaining({ defaultPath: "/home/ada/.ssh" }));
  });
});

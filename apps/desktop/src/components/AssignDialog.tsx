import { useId, useState } from "react";

import { displayPath } from "../lib/format";
import type { Profile } from "../lib/types";
import { Modal } from "./Modal";
import { Button, ErrorNotice, Mono } from "./ui";

/** Choose the profile to write into a repository's .git/config. */
export function AssignDialog({
  repository,
  home,
  profiles,
  suggested,
  busy,
  error,
  onPreview,
  onClose,
}: {
  repository: string;
  home?: string;
  profiles: Profile[];
  suggested?: string;
  busy: boolean;
  error: string | null;
  onPreview: (profile: string, ssh: boolean) => void;
  onClose: () => void;
}) {
  const [selected, setSelected] = useState(suggested ?? profiles[0]?.id ?? "");
  const [ssh, setSsh] = useState(false);
  const sshHelpId = useId();
  const chosen = profiles.find((p) => p.id === selected);

  return (
    <Modal
      title="Assign a profile"
      description={
        <>
          Writes the profile’s name and email to <Mono>{displayPath(repository, home)}/.git/config</Mono>. You’ll see
          the exact change before anything is written.
        </>
      }
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button
            variant="primary"
            disabled={!chosen || busy}
            onClick={() => chosen && onPreview(chosen.id, ssh && Boolean(chosen.ssh_key))}
          >
            {busy ? "Preparing…" : "Preview changes"}
          </Button>
        </>
      }
    >
      {profiles.length === 0 ? (
        <p>Create a profile first, then assign it here.</p>
      ) : (
        <div className="flex flex-col gap-4">
          <fieldset className="flex flex-col gap-1">
            <legend className="mb-1 text-sm font-medium">Profile</legend>
            {profiles.map((p) => (
              <label
                key={p.id}
                className={`flex cursor-pointer items-start gap-3 rounded-md border px-3 py-2 ${
                  selected === p.id ? "border-accent bg-accent-soft" : "border-line hover:bg-sidebar"
                }`}
              >
                <input
                  type="radio"
                  name="profile"
                  value={p.id}
                  checked={selected === p.id}
                  onChange={() => setSelected(p.id)}
                  className="mt-1 accent-[var(--color-accent)]"
                />
                <span className="flex min-w-0 flex-col">
                  <span className="font-medium">
                    {p.id}
                    {p.label && <span className="font-normal text-muted-text"> — {p.label}</span>}
                  </span>
                  <span className="text-sm text-muted-text">
                    {p.name} <Mono>&lt;{p.email}&gt;</Mono>
                  </span>
                </span>
              </label>
            ))}
          </fieldset>

          <label className={`flex items-start gap-3 ${chosen?.ssh_key ? "" : "opacity-60"}`}>
            <input
              type="checkbox"
              checked={ssh && Boolean(chosen?.ssh_key)}
              disabled={!chosen?.ssh_key}
              onChange={(e) => setSsh(e.target.checked)}
              aria-describedby={sshHelpId}
              className="mt-1 accent-[var(--color-accent)]"
            />
            <span className="flex flex-col">
              <span className="text-sm font-medium">Also use this profile’s SSH key for this repository</span>
              <span id={sshHelpId} className="text-sm text-muted-text">
                {chosen?.ssh_key ? (
                  <>
                    Sets <Mono>core.sshCommand</Mono> so pushes over SSH authenticate with{" "}
                    <Mono>{chosen.ssh_key}</Mono>. Your <Mono>~/.ssh/config</Mono> is not changed.
                  </>
                ) : (
                  "This profile has no SSH key. Add one to the profile to enable this."
                )}
              </span>
            </span>
          </label>
          {error && <ErrorNotice message={error} />}
        </div>
      )}
    </Modal>
  );
}

import { useId, useState } from "react";

import { api, errorMessage } from "../lib/api";
import type { Profile } from "../lib/types";
import { Modal } from "./Modal";
import { Button, ErrorNotice, Field, Mono } from "./ui";

/** Choose a folder and a profile for a new rule. */
export function RuleDialog({
  profiles,
  busy,
  error,
  onPreview,
  onClose,
}: {
  profiles: Profile[];
  busy: boolean;
  error: string | null;
  onPreview: (directory: string, profile: string) => void;
  onClose: () => void;
}) {
  const [directory, setDirectory] = useState("");
  const [profile, setProfile] = useState(profiles[0]?.id ?? "");
  const [pickError, setPickError] = useState<string | null>(null);
  const [touched, setTouched] = useState(false);
  const selectId = useId();

  const choose = async () => {
    try {
      const picked = await api.pickDirectory("Choose the folder for this rule");
      if (picked) setDirectory(picked);
    } catch (e) {
      setPickError(errorMessage(e));
    }
  };

  const dirError =
    touched && !directory.trim()
      ? "Choose a folder."
      : touched && !(directory.startsWith("/") || directory.startsWith("~/"))
        ? "Use an absolute path or one starting with ~/."
        : undefined;

  return (
    <Modal
      title="Add a folder rule"
      description={
        <>
          Every repository inside the folder will use the profile. GitMate adds an{" "}
          <Mono>includeIf "gitdir:…"</Mono> section to your global Git config; you’ll review it first.
        </>
      }
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button
            variant="primary"
            disabled={busy || profiles.length === 0}
            onClick={() => {
              setTouched(true);
              const d = directory.trim();
              if (d && (d.startsWith("/") || d.startsWith("~/")) && profile) onPreview(d, profile);
            }}
          >
            {busy ? "Preparing…" : "Preview changes"}
          </Button>
        </>
      }
    >
      {profiles.length === 0 ? (
        <p>Create a profile first; a rule applies a profile to a folder.</p>
      ) : (
        <div className="flex flex-col gap-4">
          <Field
            label="Folder"
            value={directory}
            onChange={(e) => setDirectory(e.target.value)}
            placeholder="~/work"
            hint="Repositories anywhere below this folder match, including linked worktrees of those repositories."
            error={dirError}
            spellCheck={false}
            className="font-mono text-[0.78rem]"
            trailing={<Button onClick={choose}>Choose…</Button>}
            autoFocus
          />
          <div className="flex flex-col gap-1">
            <label htmlFor={selectId} className="text-sm font-medium">
              Profile
            </label>
            <select
              id={selectId}
              value={profile}
              onChange={(e) => setProfile(e.target.value)}
              className="h-8 rounded-md border border-line-strong bg-white px-2 text-base"
            >
              {profiles.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.id} — {p.email}
                </option>
              ))}
            </select>
          </div>
          {(error || pickError) && <ErrorNotice message={error ?? pickError ?? ""} />}
        </div>
      )}
    </Modal>
  );
}

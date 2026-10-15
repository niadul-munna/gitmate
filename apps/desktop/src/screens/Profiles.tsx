import { useState } from "react";

import { Modal } from "../components/Modal";
import { ProfileForm } from "../components/ProfileForm";
import { Button, ErrorNotice, Loading, Mono, ScreenHeader } from "../components/ui";
import { useLoad } from "../hooks/useLoad";
import { api, errorMessage } from "../lib/api";
import { displayPath } from "../lib/format";
import type { Issue, Profile } from "../lib/types";
import { useApp } from "../state";

function Detail({
  id,
  onEdit,
  onDeleted,
}: {
  id: string;
  onEdit: (profile: Profile) => void;
  onDeleted: () => void;
}) {
  const { home, dataVersion, info } = useApp();
  const usage = useLoad(() => api.getProfile(id), [id], dataVersion);
  const [confirming, setConfirming] = useState(false);
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const [deleting, setDeleting] = useState(false);

  if (usage.loading && !usage.data) return <Loading />;
  if (usage.error) return <ErrorNotice message={usage.error} />;
  if (!usage.data) return null;
  const { profile, rules, repositories, ssh_key } = usage.data;

  const remove = async () => {
    setDeleting(true);
    setDeleteError(null);
    try {
      await api.deleteProfile(profile.id);
      setConfirming(false);
      onDeleted();
    } catch (e) {
      setDeleteError(errorMessage(e));
    } finally {
      setDeleting(false);
    }
  };

  return (
    <div className="flex flex-col gap-6">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h2 className="text-xl font-semibold tracking-tight">{profile.id}</h2>
          {profile.label && <p className="text-muted-text">{profile.label}</p>}
        </div>
        <div className="flex gap-2">
          <Button onClick={() => onEdit(profile)}>Edit profile</Button>
          <Button variant="danger" onClick={() => setConfirming(true)}>
            Delete profile
          </Button>
        </div>
      </div>

      <dl className="grid grid-cols-[9rem_minmax(0,1fr)] gap-x-3 gap-y-2">
        <dt className="text-sm text-muted-text">Name</dt>
        <dd>{profile.name}</dd>
        <dt className="text-sm text-muted-text">Email</dt>
        <dd>
          <Mono>{profile.email}</Mono>
        </dd>
        <dt className="text-sm text-muted-text">SSH key</dt>
        <dd>
          {ssh_key ? (
            <>
              <Mono>{displayPath(ssh_key.path, home)}</Mono>
              <div className={`text-sm ${ssh_key.status.status === "ok" ? "text-muted-text" : "text-danger"}`}>
                {ssh_key.status.status === "ok"
                  ? "Found; permissions are private"
                  : ssh_key.status.status === "missing"
                    ? "This file does not exist"
                    : ssh_key.status.status === "permissions_too_open"
                      ? `Permissions ${ssh_key.status.mode.toString(8)} are too open; ssh will refuse it (chmod 600)`
                      : "Cannot be used"}
              </div>
            </>
          ) : (
            <span className="text-muted-text">None</span>
          )}
        </dd>
        <dt className="text-sm text-muted-text">Include file</dt>
        <dd>
          <Mono className="text-muted-text">
            {info ? displayPath(`${info.paths.config_dir}/includes/${profile.id}.gitconfig`, home) : ""}
          </Mono>
        </dd>
      </dl>

      <section aria-label="Where this profile is used" className="flex flex-col gap-2">
        <h3 className="text-base font-semibold">Used by</h3>
        {rules.length === 0 && repositories.length === 0 ? (
          <p className="text-sm text-muted-text">
            Not used yet. Assign it to a repository from the Overview, or add a folder rule.
          </p>
        ) : (
          <ul className="flex flex-col divide-y divide-line border-y border-line">
            {rules.map((r) => (
              <li key={r.condition} className="flex justify-between gap-3 py-2">
                <Mono>{r.directory ? `${displayPath(r.directory, home)}/` : r.condition}</Mono>
                <span className="text-sm text-muted-text">Folder rule</span>
              </li>
            ))}
            {repositories.map((repo) => (
              <li key={repo} className="flex justify-between gap-3 py-2">
                <Mono>{displayPath(repo, home)}</Mono>
                <span className="text-sm text-muted-text">Assigned repository</span>
              </li>
            ))}
          </ul>
        )}
      </section>

      {confirming && (
        <Modal
          title={`Delete profile '${profile.id}'?`}
          width="sm"
          description="This removes it from GitMate and deletes its generated include file."
          onClose={() => setConfirming(false)}
          footer={
            <>
              <Button onClick={() => setConfirming(false)} autoFocus>
                Cancel
              </Button>
              <Button variant="danger" onClick={remove} disabled={deleting || rules.length > 0}>
                Delete
              </Button>
            </>
          }
        >
          <div className="flex flex-col gap-2 text-base">
            {rules.length > 0 ? (
              <p>
                Folder rules still use this profile. Remove {rules.length === 1 ? "that rule" : "those rules"} on the
                Rules screen first.
              </p>
            ) : (
              <p>Repositories you assigned it to keep their current name and email in .git/config.</p>
            )}
            {deleteError && <ErrorNotice message={deleteError} />}
          </div>
        </Modal>
      )}
    </div>
  );
}

export function Profiles() {
  const { dataVersion, refresh } = useApp();
  const profiles = useLoad(() => api.listProfiles(), [], dataVersion);
  const [selected, setSelected] = useState<string | null>(null);
  const [editing, setEditing] = useState<Profile | "new" | null>(null);
  const [warnings, setWarnings] = useState<Issue[]>([]);
  const list = profiles.data ?? [];
  const current = selected && list.some((p) => p.id === selected) ? selected : (list[0]?.id ?? null);

  return (
    <>
      <ScreenHeader title="Profiles">
        <Button variant="primary" onClick={() => setEditing("new")}>
          New profile
        </Button>
      </ScreenHeader>
      {profiles.error && (
        <div className="px-8 py-4">
          <ErrorNotice message={profiles.error} />
        </div>
      )}
      {profiles.loading && !profiles.data && (
        <div className="px-8 py-6">
          <Loading />
        </div>
      )}
      {profiles.data && list.length === 0 && (
        <div className="flex max-w-[60ch] flex-col gap-3 px-8 py-10">
          <h2 className="text-xl font-semibold tracking-tight">No profiles yet</h2>
          <p className="text-muted-text">
            A profile is the name and email you commit as for one account, such as work or personal, plus optionally
            the SSH key for that account.
          </p>
          <div>
            <Button variant="primary" onClick={() => setEditing("new")}>
              Create your first profile
            </Button>
          </div>
        </div>
      )}
      {list.length > 0 && (
        <div className="grid min-h-0 flex-1 grid-cols-1 md:grid-cols-[15rem_minmax(0,1fr)]">
          <nav aria-label="Profiles" className="border-b border-line md:border-r md:border-b-0">
            <ul className="flex flex-col gap-0.5 p-3">
              {list.map((p) => (
                <li key={p.id}>
                  <button
                    type="button"
                    aria-current={current === p.id ? "true" : undefined}
                    onClick={() => setSelected(p.id)}
                    className={`flex w-full flex-col items-start rounded-md px-3 py-2 text-left ${
                      current === p.id ? "bg-accent-soft" : "hover:bg-sidebar"
                    }`}
                  >
                    <span className="font-medium">{p.id}</span>
                    <span className="mono w-full truncate text-muted-text">{p.email}</span>
                  </button>
                </li>
              ))}
            </ul>
          </nav>
          <div className="max-w-[760px] px-8 py-6">
            {warnings.length > 0 && (
              <div role="status" className="mb-4 rounded-md bg-warning-soft px-3 py-2 text-sm text-warning">
                {warnings.map((w) => (
                  <p key={w.code}>{w.message}</p>
                ))}
              </div>
            )}
            {current && (
              <Detail
                id={current}
                onEdit={(p) => setEditing(p)}
                onDeleted={() => {
                  setSelected(null);
                  refresh();
                }}
              />
            )}
          </div>
        </div>
      )}
      {editing && (
        <Modal
          title={editing === "new" ? "New profile" : `Edit profile '${editing.id}'`}
          description={
            editing === "new"
              ? undefined
              : "Repositories covered by this profile’s folder rules pick up the new values right away."
          }
          onClose={() => setEditing(null)}
        >
          <ProfileForm
            profile={editing === "new" ? undefined : editing}
            onCancel={() => setEditing(null)}
            onSaved={(saved) => {
              setEditing(null);
              setSelected(saved.profile.id);
              setWarnings(saved.warnings);
              refresh();
            }}
          />
        </Modal>
      )}
    </>
  );
}

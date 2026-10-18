import { useEffect, useState, type FormEvent } from "react";

import { api, errorMessage, isCommandError } from "../lib/api";
import type { Profile, ProfileSaved, SshKeyChoices } from "../lib/types";
import { Button, ErrorNotice, Field } from "./ui";

type FieldName = "id" | "name" | "email" | "label" | "ssh_key";
type Errors = Partial<Record<FieldName, string>>;

const ID_PATTERN = /^[a-z0-9][a-z0-9_-]{0,31}$/;

function validate(values: Record<FieldName, string>, creating: boolean): Errors {
  const errors: Errors = {};
  if (creating) {
    if (!values.id.trim()) errors.id = "Required.";
    else if (!ID_PATTERN.test(values.id.trim()))
      errors.id = "Use 1–32 lowercase letters, digits, - or _, starting with a letter or digit.";
  }
  if (!values.name.trim()) errors.name = "Required.";
  if (!values.email.trim()) errors.email = "Required.";
  else if (!/^[^\s@<>]+@[^\s@<>]+$/.test(values.email.trim())) errors.email = "Enter an address like name@example.com.";
  const key = values.ssh_key.trim();
  if (key && !(key.startsWith("/") || key.startsWith("~/")))
    errors.ssh_key = "Use an absolute path or one starting with ~/.";
  return errors;
}

/**
 * Create or edit a profile. Validation runs here for quick feedback and again
 * in Rust, whose field errors are shown next to the matching field.
 */
export function ProfileForm({
  profile,
  onSaved,
  onCancel,
}: {
  profile?: Profile;
  onSaved: (saved: ProfileSaved) => void;
  onCancel: () => void;
}) {
  const creating = !profile;
  const [values, setValues] = useState<Record<FieldName, string>>({
    id: profile?.id ?? "",
    name: profile?.name ?? "",
    email: profile?.email ?? "",
    label: profile?.label ?? "",
    ssh_key: profile?.ssh_key ?? "",
  });
  const [errors, setErrors] = useState<Errors>({});
  const [formError, setFormError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [sshKeys, setSshKeys] = useState<SshKeyChoices | null>(null);

  useEffect(() => {
    let live = true;
    // Only a convenience: without the list, the field and picker still work.
    api.listSshKeys().then(
      (found) => live && setSshKeys(found),
      () => {},
    );
    return () => {
      live = false;
    };
  }, []);

  const set = (field: FieldName) => (event: { target: { value: string } }) =>
    setValues((v) => ({ ...v, [field]: event.target.value }));

  const pickKey = async () => {
    try {
      const picked = await api.pickFile("Choose the SSH private key", sshKeys?.dir);
      if (picked) setValues((v) => ({ ...v, ssh_key: picked }));
    } catch (e) {
      setFormError(errorMessage(e));
    }
  };

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    const found = validate(values, creating);
    setErrors(found);
    setFormError(null);
    if (Object.keys(found).length > 0) return;

    const fields = {
      name: values.name.trim(),
      email: values.email.trim(),
      label: values.label.trim() || null,
      ssh_key: values.ssh_key.trim() || null,
    };
    setBusy(true);
    try {
      const saved = profile
        ? await api.updateProfile(profile.id, fields)
        : await api.createProfile({ id: values.id.trim(), ...fields });
      onSaved(saved);
    } catch (e) {
      if (isCommandError(e) && e.fields.length > 0) {
        const mapped: Errors = {};
        for (const f of e.fields) mapped[f.field as FieldName] = capitalize(f.message);
        setErrors(mapped);
      } else {
        setFormError(errorMessage(e));
      }
    } finally {
      setBusy(false);
    }
  };

  return (
    <form onSubmit={submit} noValidate className="flex flex-col gap-4">
      {creating && (
        <Field
          label="Profile id"
          value={values.id}
          onChange={set("id")}
          error={errors.id}
          hint="Short name used in commands, e.g. gitmate use work."
          autoComplete="off"
          spellCheck={false}
          autoFocus
        />
      )}
      <Field
        label="Name"
        value={values.name}
        onChange={set("name")}
        error={errors.name}
        hint="Recorded as the commit author (user.name)."
        autoComplete="name"
        autoFocus={!creating}
      />
      <Field
        label="Email"
        type="email"
        value={values.email}
        onChange={set("email")}
        error={errors.email}
        hint="Recorded as the commit author email (user.email). Hosts link commits to accounts by this address."
        autoComplete="email"
        spellCheck={false}
      />
      <Field
        label="Label"
        optional
        value={values.label}
        onChange={set("label")}
        error={errors.label}
        hint="Shown in lists, e.g. Acme (work GitHub)."
      />
      <Field
        label="SSH private key"
        optional
        value={values.ssh_key}
        onChange={set("ssh_key")}
        error={errors.ssh_key}
        placeholder="~/.ssh/id_ed25519_work"
        hint="Only the path is stored. GitMate never opens the key file."
        spellCheck={false}
        className="font-mono text-[0.78rem]"
        trailing={<Button onClick={pickKey}>Choose…</Button>}
      />
      {sshKeys && sshKeys.keys.length > 0 && (
        <div className="-mt-2 flex flex-wrap items-center gap-1.5" role="group" aria-label="Keys found in ~/.ssh">
          <span className="text-sm text-muted-text">Keys in ~/.ssh:</span>
          {sshKeys.keys.map((key) => {
            const others = key.used_by.filter((id) => id !== profile?.id);
            return (
              <Button key={key.path} size="sm" onClick={() => setValues((v) => ({ ...v, ssh_key: key.path }))}>
                <span className="font-mono">{key.path.split("/").pop()}</span>
                {others.length > 0 && <span className="text-muted-text"> · used by {others.join(", ")}</span>}
              </Button>
            );
          })}
        </div>
      )}

      {formError && <ErrorNotice message={formError} />}

      <div className="flex justify-end gap-2 border-t border-line pt-3">
        <Button onClick={onCancel} disabled={busy}>
          Cancel
        </Button>
        <Button type="submit" variant="primary" disabled={busy}>
          {creating ? "Create profile" : "Save changes"}
        </Button>
      </div>
    </form>
  );
}

function capitalize(s: string): string {
  return s.charAt(0).toUpperCase() + s.slice(1) + (s.endsWith(".") ? "" : ".");
}

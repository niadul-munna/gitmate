import { useState } from "react";

import { AssignDialog } from "../components/AssignDialog";
import { FindingList } from "../components/FindingList";
import { Button, ErrorNotice, Loading, Mono, ScreenHeader, Section } from "../components/ui";
import { usePlan } from "../hooks/usePlan";
import { useLoad } from "../hooks/useLoad";
import { api } from "../lib/api";
import { displayPath, pluralize } from "../lib/format";
import type { Finding } from "../lib/types";
import { useApp } from "../state";

/** Identity mismatches and configuration problems, with fixes. */
export function Diagnostics() {
  const { repository, home, dataVersion, go } = useApp();
  const [runs, setRuns] = useState(0);
  const doctor = useLoad(() => api.runDoctor(repository), [repository], `${dataVersion}:${runs}`);
  const profiles = useLoad(() => api.listProfiles(), [], dataVersion);
  const plan = usePlan();
  const [assigning, setAssigning] = useState<string | undefined | null>(null);
  const data = doctor.data;

  const problems = data?.findings.filter((f) => f.severity !== "info") ?? [];
  const notes = data?.findings.filter((f) => f.severity === "info") ?? [];

  const action = (f: Finding) => {
    if (f.code === "profile_mismatch" && data?.repository?.expected) {
      const id = data.repository.expected.profile;
      return (
        <Button variant="primary" size="sm" onClick={() => setAssigning(id)}>
          Assign “{id}”
        </Button>
      );
    }
    if ((f.code === "identity_unset" || f.code === "identity_guessed") && data?.repository?.repository) {
      return (
        <Button size="sm" onClick={() => setAssigning(undefined)}>
          Assign a profile…
        </Button>
      );
    }
    if (f.code.startsWith("rule_") || f.code === "global_identity_after_rule" || f.code === "include_stale") {
      return (
        <Button size="sm" onClick={() => go(f.code === "include_stale" ? "profiles" : "rules")}>
          {f.code === "include_stale" ? "Open profiles" : "Open rules"}
        </Button>
      );
    }
    if (f.code === "interrupted_operation" || f.code === "failed_operation") {
      return (
        <Button size="sm" onClick={() => go("history")}>
          Open history
        </Button>
      );
    }
    return null;
  };

  return (
    <>
      <ScreenHeader title="Diagnostics">
        <Button onClick={() => setRuns((n) => n + 1)} disabled={doctor.loading}>
          {doctor.loading ? "Checking…" : "Check again"}
        </Button>
      </ScreenHeader>
      <div className="flex max-w-[960px] flex-col gap-8 px-8 py-6">
        {doctor.loading && !data && <Loading label="Checking profiles, rules and repository" />}
        {doctor.error && <ErrorNotice message={doctor.error} />}
        {data && (
          <>
            <p className="text-base" role="status">
              {problems.length === 0 ? (
                <>No problems found.</>
              ) : (
                <>
                  {pluralize(problems.length, "problem")} {problems.length === 1 ? "needs" : "need"} attention.
                </>
              )}{" "}
              <span className="text-muted-text">
                Checked {pluralize(data.profile_count, "profile")}, {pluralize(data.rules.length, "rule")}
                {data.repository
                  ? ` and ${displayPath(data.repository.repository?.work_tree ?? data.repository.path, home)}`
                  : " (choose a repository on the Overview to check it too)"}
                .
              </span>
            </p>

            {problems.length > 0 && (
              <Section title="Problems">
                <FindingList findings={problems} action={action} label="Problems" />
              </Section>
            )}
            {notes.length > 0 && (
              <Section title="Notes">
                <FindingList findings={notes} action={action} label="Notes" />
              </Section>
            )}

            <Section title="What was checked">
              <dl className="grid grid-cols-[10rem_minmax(0,1fr)] gap-x-3 gap-y-1.5 text-base">
                <dt className="text-sm text-muted-text">Git</dt>
                <dd>{data.git_version ? data.git_version.text : <span className="text-danger">Not usable</span>}</dd>
                <dt className="text-sm text-muted-text">Global Git config</dt>
                <dd>
                  <Mono>{displayPath(data.global_config, home)}</Mono>
                </dd>
                <dt className="text-sm text-muted-text">Profiles file</dt>
                <dd>
                  <Mono>{displayPath(`${data.paths.config_dir}/profiles.toml`, home)}</Mono>
                </dd>
              </dl>
              <p className="max-w-[72ch] text-sm text-muted-text">
                Environment variables such as <Mono>GIT_AUTHOR_EMAIL</Mono> are read from GitMate’s own environment,
                which can differ from your terminal. Run <Mono>gitmate doctor</Mono> in a terminal to check that
                environment.
              </p>
            </Section>
          </>
        )}
      </div>
      {assigning !== null && data?.repository && (
        <AssignDialog
          repository={data.repository.repository?.work_tree ?? data.repository.path}
          home={home}
          profiles={profiles.data ?? []}
          suggested={assigning}
          busy={plan.busy}
          error={plan.error}
          onClose={() => {
            setAssigning(null);
            plan.setError(null);
          }}
          onPreview={async (profile, ssh) => {
            const path = data.repository?.path;
            if (path && (await plan.preview(() => api.planAssign(path, profile, ssh)))) setAssigning(null);
          }}
        />
      )}
      {plan.dialog}
    </>
  );
}

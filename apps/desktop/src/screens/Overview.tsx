import { useState } from "react";

import { AssignDialog } from "../components/AssignDialog";
import { FindingList } from "../components/FindingList";
import { IdentityTrace } from "../components/IdentityTrace";
import { RepositoryPicker } from "../components/RepositoryPicker";
import { Button, ErrorNotice, Loading, Mono, ScreenHeader, Section } from "../components/ui";
import { usePlan } from "../hooks/usePlan";
import { useLoad } from "../hooks/useLoad";
import { api } from "../lib/api";
import { displayPath, sourceText } from "../lib/format";
import type { Finding, RepoReport } from "../lib/types";
import { useApp } from "../state";

function Fact({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="grid grid-cols-[8.5rem_minmax(0,1fr)] gap-3 py-1.5">
      <dt className="text-sm text-muted-text">{label}</dt>
      <dd className="min-w-0 text-base">{children}</dd>
    </div>
  );
}

export function sshSummary(report: RepoReport, home?: string): React.ReactNode {
  if (!report.repository) return "Not applicable outside a repository";
  const transports = [...new Set(report.remotes.flatMap((r) => r.fetch.map((u) => u.transport)))];
  const remoteText = transports.length ? `Remotes use ${transports.join(" and ").toUpperCase()}` : "No remotes";
  if (report.ssh.command) {
    return (
      <>
        {remoteText}. Git runs <Mono>{report.ssh.command}</Mono>{" "}
        <span className="text-sm text-muted-text">({report.ssh.command_source})</span>
      </>
    );
  }
  const key = report.ssh.profile_key;
  return (
    <>
      {remoteText}. SSH uses your default configuration
      {key ? (
        <span className="text-sm text-muted-text">
          {" "}
          (profile key <Mono>{displayPath(key.path, home)}</Mono> is not pinned here)
        </span>
      ) : null}
      .
    </>
  );
}

export function Overview() {
  const { repository, home, info, dataVersion, go } = useApp();
  const report = useLoad(repository ? () => api.inspectRepository(repository) : null, [repository], dataVersion);
  const profiles = useLoad(() => api.listProfiles(), [], dataVersion);
  const rules = useLoad(() => api.listRules(), [], dataVersion);
  const plan = usePlan();
  const [assigning, setAssigning] = useState<string | undefined | null>(null);
  const [field, setField] = useState<"email" | "name">("email");

  const assignAction = (f: Finding) => {
    if (f.code === "profile_mismatch" && report.data?.expected) {
      const id = report.data.expected.profile;
      return (
        <Button variant="primary" size="sm" onClick={() => setAssigning(id)}>
          Assign “{id}”
        </Button>
      );
    }
    if (f.code === "identity_unset" || f.code === "identity_guessed") {
      return (
        <Button size="sm" onClick={() => setAssigning(undefined)}>
          Assign a profile…
        </Button>
      );
    }
    return null;
  };

  if (!repository) {
    return (
      <>
        <ScreenHeader title="Overview" />
        <div className="flex max-w-[60ch] flex-col gap-4 px-8 py-10">
          <h2 className="text-xl font-semibold tracking-tight">Which identity will your next commit carry?</h2>
          <p className="text-base text-muted-text">
            Choose a repository to see the name and email Git will record, the exact configuration file they come
            from, and whether that matches the profile you expect.
          </p>
          <RepositoryPicker variant="primary" />
          <p className="text-sm text-muted-text">
            GitMate reads configuration through Git itself and changes nothing until you review and apply a
            change.
          </p>
        </div>
      </>
    );
  }

  const data = report.data;
  const author = data?.identity.author;

  return (
    <>
      <ScreenHeader title="Overview">
        <RepositoryPicker />
      </ScreenHeader>
      <div className="flex max-w-[960px] flex-col gap-8 px-8 py-6">
        {report.loading && !data && <Loading label="Reading Git configuration" />}
        {report.error && <ErrorNotice message={report.error} />}
        {data && (
          <>
            <section aria-label="Commit identity" className="flex flex-col gap-3">
              <p className="text-sm text-muted-text">
                {data.repository ? "Commits in " : "Outside a repository, in "}
                <Mono className="text-ink">{displayPath(data.repository?.work_tree ?? data.path, home)}</Mono>
                {data.repository?.is_linked_worktree && " (linked worktree)"}
                {data.repository?.is_bare && " (bare repository)"} are authored as
              </p>
              {author ? (
                <p className="flex flex-wrap items-baseline gap-x-3 gap-y-1">
                  <span className="text-xl font-semibold tracking-tight">{author.name}</span>
                  <span className="mono text-[0.95rem]">{author.email}</span>
                </p>
              ) : (
                <p className="text-xl font-semibold text-danger">No identity configured</p>
              )}
              <dl className="mt-1 border-t border-line">
                <Fact label="Profile">
                  {data.profile ? (
                    <>
                      {data.profile.id}
                      {data.profile.label && <span className="text-muted-text"> — {data.profile.label}</span>}
                    </>
                  ) : (
                    <span className="text-muted-text">No profile matches this email</span>
                  )}
                </Fact>
                {data.expected && (
                  <Fact label="Expected">
                    {data.expected.profile}{" "}
                    <span className="text-sm text-muted-text">
                      {data.expected.reason.kind === "rule"
                        ? `from the rule for ${displayPath(data.expected.reason.directory, home)}`
                        : "assigned to this repository"}
                    </span>
                  </Fact>
                )}
                <Fact label="Email set by">
                  {data.identity.email_source ? sourceText(data.identity.email_source, home) : "Nothing"}
                </Fact>
                <Fact label="SSH">{sshSummary(data, home)}</Fact>
              </dl>
              <div className="flex flex-wrap gap-2">
                {data.repository && (
                  <Button onClick={() => setAssigning(data.expected?.profile ?? data.profile?.id)}>
                    Assign a profile…
                  </Button>
                )}
                <Button variant="ghost" onClick={() => go("repository")}>
                  Repository details
                </Button>
              </div>
            </section>

            {data.findings.length > 0 && (
              <Section title="Needs attention">
                <FindingList findings={data.findings} action={assignAction} label="Findings for this repository" />
              </Section>
            )}

            <Section
              title="How Git decided"
              description="Every value Git found, in the order it reads them. The last one wins unless an environment variable or author setting takes precedence."
              actions={
                <div role="group" aria-label="Setting to trace" className="inline-flex rounded-md border border-line-strong p-0.5">
                  {(["email", "name"] as const).map((f) => (
                    <button
                      key={f}
                      type="button"
                      aria-pressed={field === f}
                      onClick={() => setField(f)}
                      className={`h-6 rounded px-2.5 text-sm ${
                        field === f ? "bg-accent-soft font-medium text-accent-strong" : "text-muted-text hover:text-ink"
                      }`}
                    >
                      {f === "email" ? "Email" : "Name"}
                    </button>
                  ))}
                </div>
              }
            >
              <IdentityTrace
                report={data.identity}
                field={field}
                home={home}
                includesDir={info ? `${info.paths.config_dir}/includes` : undefined}
                rules={rules.data ?? []}
              />
            </Section>
          </>
        )}
      </div>
      {assigning !== null && data && (
        <AssignDialog
          repository={data.repository?.work_tree ?? data.path}
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
            if (await plan.preview(() => api.planAssign(data.path, profile, ssh))) setAssigning(null);
          }}
        />
      )}
      {plan.dialog}
    </>
  );
}

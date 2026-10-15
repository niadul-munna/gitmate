import { RepositoryPicker } from "../components/RepositoryPicker";
import { FindingList } from "../components/FindingList";
import { ErrorNotice, Loading, Mono, ScreenHeader, Section } from "../components/ui";
import { useLoad } from "../hooks/useLoad";
import { api } from "../lib/api";
import { displayKey, displayPath, originText, scopeName, sourceText } from "../lib/format";
import type { KeyCheck, Remote, RemoteUrl } from "../lib/types";
import { useApp } from "../state";
import { sshSummary } from "./Overview";

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="grid grid-cols-[9rem_minmax(0,1fr)] gap-3 border-t border-line py-2 first:border-t-0">
      <dt className="text-sm text-muted-text">{label}</dt>
      <dd className="min-w-0">{children}</dd>
    </div>
  );
}

/** One row per URL; identical fetch and push URLs share a row. */
function remoteRows(remote: Remote): { use: string; url: RemoteUrl }[] {
  const same =
    remote.push.length === remote.fetch.length && remote.push.every((p, i) => p.display === remote.fetch[i]?.display);
  if (same) return remote.fetch.map((url) => ({ use: "Fetch, push", url }));
  return [
    ...remote.fetch.map((url) => ({ use: "Fetch", url })),
    ...remote.push.map((url) => ({ use: "Push", url })),
  ];
}

function keyStatus(check: KeyCheck): string {
  switch (check.status.status) {
    case "ok":
      return check.public_key_present ? "Found, permissions OK" : "Found, permissions OK (no .pub file next to it)";
    case "missing":
      return "Missing: the file does not exist";
    case "not_a_file":
      return "Not a regular file";
    case "permissions_too_open":
      return `Permissions ${check.status.mode.toString(8)} are too open; ssh will refuse it (use chmod 600)`;
    case "inaccessible":
      return `Cannot be inspected: ${check.status.message}`;
  }
}

/** Everything GitMate knows about one repository. */
export function Repository() {
  const { repository, home, dataVersion } = useApp();
  const report = useLoad(repository ? () => api.inspectRepository(repository) : null, [repository], dataVersion);
  const data = report.data;

  return (
    <>
      <ScreenHeader title="Repository">
        <RepositoryPicker />
      </ScreenHeader>
      <div className="flex max-w-[960px] flex-col gap-8 px-8 py-6">
        {!repository && <p className="text-muted-text">Choose a repository to see its details.</p>}
        {report.loading && !data && <Loading label="Reading Git configuration" />}
        {report.error && <ErrorNotice message={report.error} />}
        {data && (
          <>
            <Section title="Location">
              <dl>
                <Row label="Path">
                  <Mono>{displayPath(data.repository?.work_tree ?? data.path, home)}</Mono>
                </Row>
                {data.repository ? (
                  <>
                    <Row label="Kind">
                      {data.repository.is_bare
                        ? "Bare repository"
                        : data.repository.is_linked_worktree
                          ? "Linked worktree (shares configuration with its main repository)"
                          : "Repository"}
                    </Row>
                    <Row label="Git directory">
                      <Mono>{displayPath(data.repository.git_dir, home)}</Mono>
                    </Row>
                    {data.repository.common_dir !== data.repository.git_dir && (
                      <Row label="Shared directory">
                        <Mono>{displayPath(data.repository.common_dir, home)}</Mono>
                      </Row>
                    )}
                    <Row label="Local config">
                      <Mono>{displayPath(`${data.repository.common_dir}/config`, home)}</Mono>
                    </Row>
                  </>
                ) : (
                  <Row label="Kind">Not inside a Git repository</Row>
                )}
              </dl>
            </Section>

            <Section title="Commit identity">
              <dl>
                <Row label="Author">
                  {data.identity.author ? (
                    <span>
                      {data.identity.author.name} <Mono>&lt;{data.identity.author.email}&gt;</Mono>
                    </span>
                  ) : (
                    <span className="text-danger">{data.identity.problem ?? "Not configured"}</span>
                  )}
                </Row>
                {data.identity.committer &&
                  (data.identity.committer.email !== data.identity.author?.email ||
                    data.identity.committer.name !== data.identity.author?.name) && (
                    <Row label="Committer">
                      {data.identity.committer.name} <Mono>&lt;{data.identity.committer.email}&gt;</Mono>
                    </Row>
                  )}
                <Row label="Name set by">{data.identity.name_source ? sourceText(data.identity.name_source, home) : "—"}</Row>
                <Row label="Email set by">
                  {data.identity.email_source ? sourceText(data.identity.email_source, home) : "—"}
                </Row>
                <Row label="Matching profile">
                  {data.profile ? data.profile.id : <span className="text-muted-text">None</span>}
                </Row>
              </dl>
            </Section>

            <Section
              title="Configuration sources"
              description="Each identity setting Git found, in read order. For single-value settings the last one is used."
            >
              {data.identity.keys.length === 0 ? (
                <p className="text-sm text-muted-text">No identity settings are configured.</p>
              ) : (
                <table className="w-full table-fixed text-left">
                  <colgroup>
                    <col className="w-[22%]" />
                    <col className="w-[14%]" />
                    <col className="w-[38%]" />
                    <col className="w-[26%]" />
                  </colgroup>
                  <thead>
                    <tr className="text-xs text-muted-text">
                      <th scope="col" className="pb-1 font-medium">Setting</th>
                      <th scope="col" className="pb-1 font-medium">Scope</th>
                      <th scope="col" className="pb-1 font-medium">File</th>
                      <th scope="col" className="pb-1 font-medium">Value</th>
                    </tr>
                  </thead>
                  <tbody>
                    {data.identity.keys.flatMap((k) =>
                      k.values.map((v, i) => (
                        <tr key={`${k.key}-${i}`} className="border-t border-line align-top">
                          <td className="py-1.5 pr-2">
                            <Mono>{displayKey(k.key)}</Mono>
                          </td>
                          <td className="py-1.5 pr-2 text-sm">{scopeName(v.scope)}</td>
                          <td className="py-1.5 pr-2">
                            <Mono className="text-muted-text">{originText(v.origin, home)}</Mono>
                          </td>
                          <td className="py-1.5">
                            <Mono className={i === k.values.length - 1 ? "font-medium" : "text-muted-text"}>{v.value}</Mono>
                          </td>
                        </tr>
                      )),
                    )}
                  </tbody>
                </table>
              )}
              {data.identity.env.length > 0 && (
                <div className="flex flex-col gap-1">
                  <h3 className="text-sm font-medium">Environment of this app</h3>
                  <p className="text-sm text-muted-text">
                    These variables are set for GitMate. Your terminal may have different ones; run{" "}
                    <Mono>gitmate whoami</Mono> there to compare.
                  </p>
                  <ul className="flex flex-col gap-0.5">
                    {data.identity.env.map((v) => (
                      <li key={v.name}>
                        <Mono>
                          {v.name}={v.value ?? "(hidden: may contain secrets)"}
                        </Mono>
                      </li>
                    ))}
                  </ul>
                </div>
              )}
            </Section>

            {data.repository && (
              <Section
                title="Remotes and authentication"
                description="The commit identity above only labels commits. Which account can push is decided by the SSH key (SSH remotes) or the credential helper (HTTPS remotes)."
              >
                {data.remotes.length === 0 ? (
                  <p className="text-sm text-muted-text">This repository has no remotes.</p>
                ) : (
                  <table className="w-full table-fixed text-left">
                    <colgroup>
                      <col className="w-[16%]" />
                      <col className="w-[12%]" />
                      <col className="w-[12%]" />
                      <col className="w-[60%]" />
                    </colgroup>
                    <thead>
                      <tr className="text-xs text-muted-text">
                        <th scope="col" className="pb-1 font-medium">Remote</th>
                        <th scope="col" className="pb-1 font-medium">Use</th>
                        <th scope="col" className="pb-1 font-medium">Transport</th>
                        <th scope="col" className="pb-1 font-medium">URL</th>
                      </tr>
                    </thead>
                    <tbody>
                      {data.remotes.flatMap((r) =>
                        remoteRows(r).map(({ use, url }, i) => (
                          <tr key={`${r.name}-${i}`} className="border-t border-line align-top">
                            <td className="py-1.5 pr-2 text-sm">{r.name}</td>
                            <td className="py-1.5 pr-2 text-sm">{use}</td>
                            <td className="py-1.5 pr-2 text-sm uppercase">{url.transport}</td>
                            <td className="py-1.5">
                              <Mono>{url.display}</Mono>
                            </td>
                          </tr>
                        )),
                      )}
                    </tbody>
                  </table>
                )}
                <dl>
                  <Row label="SSH">{sshSummary(data, home)}</Row>
                  {data.ssh.profile_key && (
                    <Row label="Profile SSH key">
                      <Mono>{displayPath(data.ssh.profile_key.path, home)}</Mono>
                      <div className="text-sm text-muted-text">{keyStatus(data.ssh.profile_key)}</div>
                    </Row>
                  )}
                  <Row label="HTTPS credentials">
                    {data.credentials.filter(([k]) => k.endsWith("helper")).length === 0 ? (
                      <span className="text-sm text-muted-text">No credential helper is configured.</span>
                    ) : (
                      <ul>
                        {data.credentials
                          .filter(([k]) => k.endsWith("helper"))
                          .map(([k, e], i) => (
                            <li key={i}>
                              <Mono>
                                {k} = {e.value}
                              </Mono>{" "}
                              <span className="text-sm text-muted-text">({scopeName(e.scope)})</span>
                            </li>
                          ))}
                      </ul>
                    )}
                    <p className="mt-1 text-sm text-muted-text">
                      GitMate never reads stored passwords or tokens and does not switch HTTPS accounts.
                    </p>
                  </Row>
                </dl>
              </Section>
            )}

            <Section title="Findings">
              {data.findings.length === 0 ? (
                <p className="text-sm text-muted-text">No problems found for this repository.</p>
              ) : (
                <FindingList findings={data.findings} />
              )}
            </Section>
          </>
        )}
      </div>
    </>
  );
}

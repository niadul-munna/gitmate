import { useState } from "react";

import { RuleDialog } from "../components/RuleDialog";
import { Button, ErrorNotice, Loading, Mono, ScreenHeader } from "../components/ui";
import { usePlan } from "../hooks/usePlan";
import { useLoad } from "../hooks/useLoad";
import { api } from "../lib/api";
import { displayPath } from "../lib/format";
import { useApp } from "../state";

/** Folder rules: includeIf "gitdir:..." sections in the global Git config. */
export function Rules() {
  const { home, dataVersion, info } = useApp();
  const rules = useLoad(() => api.listRules(), [], dataVersion);
  const profiles = useLoad(() => api.listProfiles(), [], dataVersion);
  const plan = usePlan();
  const [adding, setAdding] = useState(false);
  const list = rules.data ?? [];

  return (
    <>
      <ScreenHeader title="Rules">
        <Button variant="primary" onClick={() => setAdding(true)}>
          Add rule
        </Button>
      </ScreenHeader>
      <div className="flex max-w-[960px] flex-col gap-5 px-8 py-6">
        <p className="max-w-[72ch] text-muted-text">
          Repositories inside a rule’s folder use its profile. Rules are <Mono>includeIf</Mono> sections in{" "}
          <Mono>{info ? displayPath(info.global_config, home) : "your global Git config"}</Mono>. When folders
          overlap, the rule written later wins, and settings in a repository’s own <Mono>.git/config</Mono> win over
          any rule.
        </p>
        {rules.loading && !rules.data && <Loading />}
        {rules.error && <ErrorNotice message={rules.error} />}
        {plan.error && !adding && <ErrorNotice message={plan.error} />}
        {rules.data && list.length === 0 && (
          <div className="flex flex-col gap-2 py-4">
            <h2 className="text-lg font-semibold">No folder rules</h2>
            <p className="text-muted-text">
              Add one to use a profile automatically for every repository in a folder, such as your work projects.
            </p>
          </div>
        )}
        {list.length > 0 && (
          <table className="w-full table-fixed text-left">
            <colgroup>
              <col className="w-[46%]" />
              <col className="w-[34%]" />
              <col className="w-[20%]" />
            </colgroup>
            <thead>
              <tr className="text-xs text-muted-text">
                <th scope="col" className="pb-1.5 font-medium">Folder</th>
                <th scope="col" className="pb-1.5 font-medium">Profile</th>
                <th scope="col" className="pb-1.5 font-medium">
                  <span className="sr-only">Actions</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {list.map((r) => (
                <tr key={`${r.file}-${r.condition}-${r.position}`} className="border-t border-line align-top">
                  <td className="py-2.5 pr-3">
                    <Mono>{r.directory ? `${displayPath(r.directory, home)}/` : r.condition}</Mono>
                  </td>
                  <td className="py-2.5 pr-3">
                    {r.profile ? (
                      <span className="font-medium">{r.profile}</span>
                    ) : (
                      <span className="flex flex-col">
                        <span className="text-sm text-muted-text">Not managed by GitMate</span>
                        <Mono className="text-muted-text">{displayPath(r.include_path, home)}</Mono>
                      </span>
                    )}
                  </td>
                  <td className="py-2 text-right">
                    {r.profile && r.directory && (
                      <Button
                        size="sm"
                        onClick={() => r.directory && plan.preview(() => api.planRemoveRule(r.directory as string))}
                        aria-label={`Remove rule for ${displayPath(r.directory, home)}`}
                      >
                        Remove
                      </Button>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
      {adding && (
        <RuleDialog
          profiles={profiles.data ?? []}
          busy={plan.busy}
          error={plan.error}
          onClose={() => {
            setAdding(false);
            plan.setError(null);
          }}
          onPreview={async (directory, profile) => {
            if (await plan.preview(() => api.planAddRule(directory, profile))) setAdding(false);
          }}
        />
      )}
      {plan.dialog}
    </>
  );
}

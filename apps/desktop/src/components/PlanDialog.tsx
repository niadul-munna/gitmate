import { useState } from "react";

import { api, isCommandError, errorMessage } from "../lib/api";
import { displayKey, displayPath } from "../lib/format";
import type { ApplyOutcome, ConfigChange, PlanTicket } from "../lib/types";
import { Modal } from "./Modal";
import { Button, ErrorNotice, Mono } from "./ui";

function Values({ values, tone }: { values: string[]; tone: "before" | "after" }) {
  if (values.length === 0) return <span className="text-sm text-muted-text italic">Not set</span>;
  return (
    <div className="flex flex-col gap-0.5">
      {values.map((v, i) => (
        <Mono key={i} className={tone === "after" ? "text-accent-strong" : "text-muted-text"}>
          {v}
        </Mono>
      ))}
    </div>
  );
}

function ChangeTable({ changes }: { changes: ConfigChange[] }) {
  return (
    <table className="w-full table-fixed border-collapse text-left">
      <colgroup>
        <col className="w-[30%]" />
        <col className="w-[35%]" />
        <col className="w-[35%]" />
      </colgroup>
      <thead>
        <tr className="text-xs text-muted-text">
          <th scope="col" className="pb-1 font-medium">
            Setting
          </th>
          <th scope="col" className="pb-1 font-medium">
            Now
          </th>
          <th scope="col" className="pb-1 font-medium">
            After
          </th>
        </tr>
      </thead>
      <tbody>
        {changes.map((c) => (
          <tr key={c.key} className="border-t border-line align-top">
            <th scope="row" className="py-2 pr-3 font-normal">
              <Mono>{displayKey(c.key)}</Mono>
            </th>
            <td className="py-2 pr-3">
              <Values values={c.before} tone="before" />
            </td>
            <td className="py-2">
              <Values values={c.after} tone="after" />
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

/**
 * Preview of a planned change: affected files, current and proposed values,
 * conflicts and warnings. Nothing is written until "Apply changes".
 */
export function PlanDialog({
  ticket,
  home,
  onClose,
}: {
  ticket: PlanTicket;
  home?: string;
  onClose: (applied: boolean) => void;
}) {
  const { plan, token } = ticket;
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<{ message: string; details: string[] } | null>(null);
  const [outcome, setOutcome] = useState<ApplyOutcome | null>(null);

  const files = [...new Set(plan.changes.map((c) => c.file))];
  const blocked = plan.conflicts.length > 0;
  const noop = plan.changes.length === 0;

  const cancel = () => {
    if (outcome) {
      onClose(true);
      return;
    }
    api.discardPlan(token).catch(() => undefined);
    onClose(false);
  };

  const apply = async () => {
    setBusy(true);
    setError(null);
    try {
      setOutcome(await api.applyPlan(token));
    } catch (e) {
      setError({ message: errorMessage(e), details: isCommandError(e) ? e.details : [] });
    } finally {
      setBusy(false);
    }
  };

  if (outcome) {
    return (
      <Modal
        title={plan.summary}
        onClose={cancel}
        footer={
          <Button variant="primary" onClick={() => onClose(true)} autoFocus>
            Done
          </Button>
        }
      >
        <div role="status" className="flex flex-col gap-2 text-base">
          <p>
            Applied.{" "}
            {outcome.operation && (
              <>
                Recorded as <Mono>{outcome.operation.id}</Mono>; you can undo it from History.
              </>
            )}
          </p>
          {outcome.effective && (
            <p>
              Commits in this repository will be authored as {outcome.effective.name} &lt;
              {outcome.effective.email}&gt;.
            </p>
          )}
          {outcome.notes.map((note) => (
            <p key={note} className="rounded-md bg-warning-soft px-3 py-2 text-sm text-warning">
              {note}
            </p>
          ))}
        </div>
      </Modal>
    );
  }

  return (
    <Modal
      title={plan.summary}
      width="lg"
      description={
        noop
          ? undefined
          : `Review the exact changes below. Nothing is written until you apply them${
              blocked ? "" : ", and every change can be undone from History"
            }.`
      }
      onClose={cancel}
      footer={
        noop ? (
          <Button variant="primary" onClick={cancel} autoFocus>
            Close
          </Button>
        ) : (
          <>
            <Button onClick={cancel} disabled={busy}>
              Cancel
            </Button>
            <Button variant="primary" onClick={apply} disabled={blocked || busy} autoFocus={!blocked}>
              {busy ? "Applying…" : "Apply changes"}
            </Button>
          </>
        )
      }
    >
      <div className="flex flex-col gap-5">
        {noop && plan.conflicts.length === 0 && (
          <p className="text-base">Already up to date. The configuration matches, so there is nothing to change.</p>
        )}

        {blocked && (
          <div className="rounded-md border border-danger/30 bg-danger-soft px-3 py-2">
            <h3 className="text-sm font-semibold text-danger">Can’t apply this change</h3>
            <ul className="mt-1 flex list-disc flex-col gap-1 pl-5 text-sm text-danger">
              {plan.conflicts.map((c) => (
                <li key={c.code + c.message}>{c.message}</li>
              ))}
            </ul>
          </div>
        )}

        {files.map((file) => (
          <section key={file} className="flex flex-col gap-1.5">
            <h3 className="text-sm text-muted-text">
              <span className="sr-only">File: </span>
              <Mono className="text-ink">{displayPath(file, home)}</Mono>
            </h3>
            <ChangeTable changes={plan.changes.filter((c) => c.file === file)} />
          </section>
        ))}

        {plan.ensure_include && (
          <p className="text-sm text-muted-text">
            The rule includes the profile file generated by GitMate for “{plan.ensure_include}”, so later edits
            to that profile apply automatically.
          </p>
        )}

        {plan.warnings.length > 0 && !noop && (
          <div className="flex flex-col gap-1">
            <h3 className="text-sm font-semibold text-warning">Before you apply</h3>
            <ul className="flex list-disc flex-col gap-1 pl-5 text-sm">
              {plan.warnings.map((w) => (
                <li key={w.code + w.message}>{w.message}</li>
              ))}
            </ul>
          </div>
        )}

        {error && <ErrorNotice message={error.message} details={error.details} />}
      </div>
    </Modal>
  );
}

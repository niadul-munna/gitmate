import { Button, ErrorNotice, Loading, Mono, ScreenHeader } from "../components/ui";
import { usePlan } from "../hooks/usePlan";
import { useLoad } from "../hooks/useLoad";
import { api } from "../lib/api";
import { displayPath, formatTime } from "../lib/format";
import type { OperationStatus } from "../lib/types";
import { useApp } from "../state";

const statusText: Record<OperationStatus, { label: string; tone: string }> = {
  applied: { label: "Applied", tone: "text-accent-strong" },
  undone: { label: "Undone", tone: "text-muted-text" },
  failed: { label: "Failed", tone: "text-danger" },
  pending: { label: "Interrupted", tone: "text-warning" },
};

/** Every change GitMate made, with undo. */
export function History() {
  const { home, dataVersion } = useApp();
  const history = useLoad(() => api.listHistory(), [], dataVersion);
  const plan = usePlan();
  const ops = [...(history.data ?? [])].reverse();

  return (
    <>
      <ScreenHeader title="History" />
      <div className="flex max-w-[960px] flex-col gap-5 px-8 py-6">
        <p className="max-w-[72ch] text-muted-text">
          Undo restores only the settings GitMate changed. If one of them was edited elsewhere since, undo stops
          and shows what changed instead of overwriting it.
        </p>
        {history.loading && !history.data && <Loading />}
        {history.error && <ErrorNotice message={history.error} />}
        {plan.error && <ErrorNotice message={plan.error} />}
        {history.data && ops.length === 0 && (
          <p className="text-muted-text">GitMate hasn’t changed any Git configuration yet.</p>
        )}
        {ops.length > 0 && (
          <ol aria-label="Changes" className="flex flex-col divide-y divide-line border-y border-line">
            {ops.map((op) => {
              const status = statusText[op.status];
              const undoable = (op.status === "applied" || op.status === "pending") && op.action.kind !== "undo";
              const files = [...new Set(op.changes.map((c) => c.file))];
              return (
                <li key={op.id} className="flex flex-wrap items-start justify-between gap-3 py-3">
                  <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                    <p className="font-medium">{op.summary}</p>
                    <p className="flex flex-wrap gap-x-3 text-sm text-muted-text">
                      <span className={status.tone}>{status.label}</span>
                      <time dateTime={new Date(op.created_at * 1000).toISOString()}>{formatTime(op.created_at)}</time>
                    </p>
                    {files.map((f) => (
                      <Mono key={f} className="text-muted-text">
                        {displayPath(f, home)}
                      </Mono>
                    ))}
                    {op.error && <p className="text-sm text-danger">{op.error}</p>}
                  </div>
                  {undoable && (
                    <Button size="sm" onClick={() => plan.preview(() => api.planUndo(op.id))} disabled={plan.busy}>
                      Undo…
                    </Button>
                  )}
                </li>
              );
            })}
          </ol>
        )}
      </div>
      {plan.dialog}
    </>
  );
}

import type { ReactNode } from "react";

import type { Finding } from "../lib/types";
import { RichText, SeverityTag } from "./ui";

/** Diagnostics with their fixes. `action` can add a button for a finding. */
export function FindingList({
  findings,
  action,
  label = "Findings",
}: {
  findings: Finding[];
  action?: (finding: Finding) => ReactNode;
  label?: string;
}) {
  if (findings.length === 0) return null;
  return (
    <ul aria-label={label} className="flex flex-col divide-y divide-line border-y border-line">
      {findings.map((f) => (
        <li key={f.code + f.message} className="flex flex-wrap items-start justify-between gap-x-4 gap-y-2 py-3">
          <div className="flex max-w-[72ch] min-w-0 flex-1 flex-col gap-1">
            <SeverityTag severity={f.severity} />
            <p className="text-base">
              <RichText text={f.message} />
            </p>
            {f.hint && (
              <p className="text-sm text-muted-text">
                <RichText text={f.hint} />
              </p>
            )}
          </div>
          {action?.(f)}
        </li>
      ))}
    </ul>
  );
}

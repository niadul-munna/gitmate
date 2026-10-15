import { useId } from "react";

import { displayPath, scopeName } from "../lib/format";
import type { ConfigEntry, IdentityReport, Rule } from "../lib/types";
import { Mono } from "./ui";

interface Step {
  key: string;
  layer: string;
  where: string;
  note?: string;
  value: string | null;
  effective: boolean;
}

function configSteps(
  report: IdentityReport,
  keyName: string,
  effectiveKey: string | null,
  home: string | undefined,
  includesDir: string | undefined,
  rules: Rule[],
): Step[] {
  const entries: ConfigEntry[] = report.keys.find((k) => k.key === keyName)?.values ?? [];
  return entries.map((entry, i) => {
    const file = entry.origin.kind === "file" ? entry.origin.detail : null;
    let note: string | undefined;
    if (file && includesDir && file.startsWith(includesDir + "/")) {
      const rule = rules.find((r) => r.include_path === file);
      note = rule?.directory
        ? `GitMate profile, included by the rule for ${displayPath(rule.directory, home)}`
        : "GitMate profile include file";
    }
    return {
      key: `${keyName}-${i}`,
      layer: scopeName(entry.scope),
      where:
        entry.origin.kind === "file"
          ? displayPath(entry.origin.detail, home)
          : entry.origin.kind === "command_line"
            ? "git -c or GIT_CONFIG_* variables"
            : entry.origin.kind,
      note,
      value: entry.value,
      effective: effectiveKey === keyName && i === entries.length - 1,
    };
  });
}

/**
 * "How Git decided": every value of the identity key in the order Git reads
 * it, ending with the one in effect. Built from `git config --show-origin`
 * and `git var`; nothing here is inferred by GitMate.
 */
export function IdentityTrace({
  report,
  field,
  home,
  includesDir,
  rules = [],
}: {
  report: IdentityReport;
  field: "email" | "name";
  home?: string;
  includesDir?: string;
  rules?: Rule[];
}) {
  const headingId = useId();
  const source = field === "email" ? report.email_source : report.name_source;
  const envVar = `GIT_AUTHOR_${field.toUpperCase()}`;
  const effectiveKey = source?.kind === "config" ? source.key : null;

  const steps: Step[] = [];
  if (field === "email") {
    const email = report.env.find((v) => v.name === "EMAIL");
    if (email) {
      steps.push({
        key: "env-EMAIL",
        layer: "Environment",
        where: "EMAIL",
        value: email.value,
        effective: source?.kind === "environment" && source.var === "EMAIL",
      });
    }
  }
  steps.push(...configSteps(report, `user.${field}`, effectiveKey, home, includesDir, rules));
  steps.push(...configSteps(report, `author.${field}`, effectiveKey, home, includesDir, rules));
  const env = report.env.find((v) => v.name === envVar);
  if (env) {
    steps.push({
      key: `env-${envVar}`,
      layer: "Environment",
      where: envVar,
      value: env.value,
      effective: source?.kind === "environment" && source.var === envVar,
    });
  }

  return (
    <div className="flex flex-col gap-2">
      <h3 id={headingId} className="sr-only">
        How Git decided the {field}
      </h3>
      {steps.length === 0 ? (
        <p className="text-sm text-muted-text">
          No {field === "email" ? "user.email" : "user.name"} is configured anywhere Git looks.
        </p>
      ) : (
        <ol aria-label={`How Git decided`} className="relative flex flex-col">
          {steps.map((step, i) => (
            <li
              key={step.key}
              className={`relative grid grid-cols-[1.25rem_minmax(0,1fr)_minmax(0,auto)] gap-x-3 py-2.5 ${
                i > 0 ? "border-t border-line" : ""
              }`}
            >
              <span aria-hidden="true" className="relative flex justify-center pt-1">
                {i < steps.length - 1 && (
                  <span className="absolute top-3.5 -bottom-6 left-1/2 w-px -translate-x-1/2 bg-line-strong" />
                )}
                <span
                  className={`relative size-2.5 rounded-full border-2 ${
                    step.effective ? "border-accent bg-accent" : "border-line-strong bg-canvas"
                  }`}
                />
              </span>
              <div className="min-w-0">
                <div className={`text-sm font-medium ${step.effective ? "" : "text-muted-text"}`}>{step.layer}</div>
                <Mono className={step.effective ? "text-ink" : "text-muted-text"}>{step.where}</Mono>
                {step.note && <div className="text-xs text-muted-text">{step.note}</div>}
              </div>
              <div className="flex min-w-0 flex-col items-end text-right">
                <Mono className={step.effective ? "font-medium text-ink" : "text-muted-text"}>
                  {step.value ?? "(value hidden)"}
                </Mono>
                <span className={`text-xs ${step.effective ? "font-medium text-accent-strong" : "text-muted-text"}`}>
                  {step.effective ? "In effect" : "Overridden"}
                </span>
              </div>
            </li>
          ))}
        </ol>
      )}
    </div>
  );
}

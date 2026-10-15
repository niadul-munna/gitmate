import { useId, type ButtonHTMLAttributes, type InputHTMLAttributes, type ReactNode } from "react";

import type { Severity } from "../lib/types";

type Variant = "primary" | "secondary" | "ghost" | "danger";

const variants: Record<Variant, string> = {
  primary: "bg-accent text-white hover:bg-accent-strong disabled:bg-line-strong disabled:text-white",
  secondary: "bg-white text-ink border border-line-strong hover:bg-sidebar disabled:text-muted",
  ghost: "text-ink hover:bg-sidebar disabled:text-muted",
  danger: "bg-white text-danger border border-line-strong hover:bg-danger-soft disabled:text-muted",
};

export function Button({
  variant = "secondary",
  size = "md",
  className = "",
  type = "button",
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & { variant?: Variant; size?: "sm" | "md" }) {
  const sizing = size === "sm" ? "h-7 px-2.5 text-sm" : "h-8 px-3 text-sm";
  return (
    <button
      type={type}
      className={`inline-flex shrink-0 items-center justify-center gap-1.5 rounded-md font-medium whitespace-nowrap transition-colors disabled:cursor-not-allowed ${sizing} ${variants[variant]} ${className}`}
      {...rest}
    />
  );
}

/** Monospace text for paths, keys and values. Wraps long paths. */
export function Mono({ children, className = "" }: { children: ReactNode; className?: string }) {
  return <code className={`mono break-all ${className}`}>{children}</code>;
}

export function Field({
  label,
  hint,
  error,
  optional,
  trailing,
  ...input
}: InputHTMLAttributes<HTMLInputElement> & {
  label: string;
  hint?: string;
  error?: string;
  optional?: boolean;
  /** A control shown beside the input, e.g. a "Choose…" button. */
  trailing?: ReactNode;
}) {
  const id = useId();
  const hintId = `${id}-hint`;
  const errorId = `${id}-error`;
  const describedBy = [hint ? hintId : null, error ? errorId : null].filter(Boolean).join(" ") || undefined;
  return (
    <div className="flex flex-col gap-1">
      <label htmlFor={id} className="text-sm font-medium">
        {label}
        {optional && <span className="font-normal text-muted-text"> (optional)</span>}
      </label>
      <div className="flex gap-2">
        <input
          id={id}
          aria-invalid={error ? true : undefined}
          aria-describedby={describedBy}
          {...input}
          className={`h-8 min-w-0 flex-1 rounded-md border bg-white px-2.5 text-base outline-none placeholder:text-muted focus-visible:border-accent ${
            error ? "border-danger" : "border-line-strong"
          } ${input.className ?? ""}`}
        />
        {trailing}
      </div>
      {hint && (
        <p id={hintId} className="text-xs text-muted-text">
          {hint}
        </p>
      )}
      {error && (
        <p id={errorId} className="text-xs text-danger">
          {error}
        </p>
      )}
    </div>
  );
}

const severityStyles: Record<Severity, { label: string; dot: string; text: string }> = {
  error: { label: "Problem", dot: "bg-danger", text: "text-danger" },
  warning: { label: "Warning", dot: "bg-warning", text: "text-warning" },
  info: { label: "Note", dot: "bg-muted", text: "text-muted-text" },
};

export function SeverityTag({ severity }: { severity: Severity }) {
  const s = severityStyles[severity];
  return (
    <span className={`inline-flex items-center gap-1.5 text-xs font-medium ${s.text}`}>
      <span aria-hidden="true" className={`size-1.5 rounded-full ${s.dot}`} />
      {s.label}
    </span>
  );
}

export function ScreenHeader({ title, children }: { title: string; children?: ReactNode }) {
  return (
    <header className="flex flex-wrap items-center justify-between gap-3 border-b border-line px-8 py-4">
      <h1 className="text-xl font-semibold tracking-tight">{title}</h1>
      {children && <div className="flex flex-wrap items-center gap-2">{children}</div>}
    </header>
  );
}

export function Section({
  title,
  description,
  actions,
  children,
}: {
  title: string;
  description?: ReactNode;
  actions?: ReactNode;
  children: ReactNode;
}) {
  const id = useId();
  return (
    <section aria-labelledby={id} className="flex flex-col gap-3">
      <div className="flex flex-wrap items-end justify-between gap-2">
        <div>
          <h2 id={id} className="text-lg font-semibold">
            {title}
          </h2>
          {description && <p className="mt-0.5 max-w-[68ch] text-sm text-muted-text">{description}</p>}
        </div>
        {actions}
      </div>
      {children}
    </section>
  );
}

export function ErrorNotice({ message, details }: { message: string; details?: string[] }) {
  return (
    <div role="alert" className="rounded-md border border-danger/30 bg-danger-soft px-3 py-2 text-sm text-danger">
      <p>{message}</p>
      {details && details.length > 0 && (
        <ul className="mt-1 list-disc pl-5">
          {details.map((d) => (
            <li key={d} className="mono">
              {d}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

export function Loading({ label = "Loading" }: { label?: string }) {
  return (
    <p role="status" className="text-sm text-muted-text">
      {label}…
    </p>
  );
}

/**
 * Render text where `backticked` spans are commands, shown in monospace.
 * Used for diagnostic hints that come from the core.
 */
export function RichText({ text }: { text: string }) {
  const parts = text.split(/(`[^`]+`)/g);
  return (
    <>
      {parts.map((part, i) =>
        part.startsWith("`") && part.endsWith("`") ? (
          <code key={i} className="mono rounded bg-sidebar px-1 py-px">
            {part.slice(1, -1)}
          </code>
        ) : (
          <span key={i}>{part}</span>
        ),
      )}
    </>
  );
}

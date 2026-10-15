/** The GitMate mark: two identities merging into one commit. */
export function Logo({ className = "" }: { className?: string }) {
  return (
    <svg viewBox="0 0 1024 1024" className={className} aria-hidden="true" focusable="false">
      <rect x="64" y="64" width="896" height="896" rx="208" fill="#526A59" />
      <g fill="none" stroke="#FAFAF8" strokeWidth="56" strokeLinecap="round">
        <path d="M352 288 L512 520 L512 724" />
        <path d="M623 359 L512 520" />
      </g>
      <circle cx="352" cy="288" r="84" fill="#FAFAF8" />
      <circle cx="672" cy="288" r="62" fill="none" stroke="#FAFAF8" strokeWidth="48" />
      <circle cx="512" cy="744" r="96" fill="#FAFAF8" />
    </svg>
  );
}

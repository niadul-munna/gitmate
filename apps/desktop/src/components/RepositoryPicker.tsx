import { useId, useState } from "react";

import { api, errorMessage } from "../lib/api";
import { displayPath } from "../lib/format";
import { useApp } from "../state";
import { Button } from "./ui";

/** Choose the repository to inspect: native folder picker plus recents. */
export function RepositoryPicker({ variant = "secondary" }: { variant?: "primary" | "secondary" }) {
  const { repository, setRepository, recents, home } = useApp();
  const [error, setError] = useState<string | null>(null);
  const recentId = useId();
  const others = recents.filter((r) => r !== repository);

  const choose = async () => {
    setError(null);
    try {
      const picked = await api.pickDirectory("Choose a repository");
      if (picked) setRepository(picked);
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  return (
    <div className="flex flex-wrap items-center gap-2">
      {others.length > 0 && (
        <>
          <label htmlFor={recentId} className="sr-only">
            Recent repositories
          </label>
          <select
            id={recentId}
            value=""
            onChange={(e) => e.target.value && setRepository(e.target.value)}
            className="h-8 max-w-[16rem] rounded-md border border-line-strong bg-white px-2 text-sm"
          >
            <option value="">Recent…</option>
            {others.map((r) => (
              <option key={r} value={r}>
                {displayPath(r, home)}
              </option>
            ))}
          </select>
        </>
      )}
      <Button variant={variant} onClick={choose}>
        Choose repository…
      </Button>
      {error && (
        <p role="alert" className="w-full text-sm text-danger">
          {error}
        </p>
      )}
    </div>
  );
}

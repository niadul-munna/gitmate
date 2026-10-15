import { useState } from "react";

import { PlanDialog } from "../components/PlanDialog";
import { errorMessage } from "../lib/api";
import type { PlanTicket } from "../lib/types";
import { useApp } from "../state";

/**
 * Request a plan from the backend and show it in the change preview. Screens
 * render `dialog` and call `preview(() => api.planSomething(...))`.
 */
export function usePlan() {
  const { home, refresh } = useApp();
  const [ticket, setTicket] = useState<PlanTicket | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const preview = async (make: () => Promise<PlanTicket>) => {
    setBusy(true);
    setError(null);
    try {
      setTicket(await make());
      return true;
    } catch (e) {
      setError(errorMessage(e));
      return false;
    } finally {
      setBusy(false);
    }
  };

  const dialog = ticket ? (
    <PlanDialog
      ticket={ticket}
      home={home}
      onClose={(applied) => {
        setTicket(null);
        if (applied) refresh();
      }}
    />
  ) : null;

  return { preview, dialog, error, setError, busy };
}

import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from "react";

import { api, errorMessage } from "./lib/api";
import type { AppInfo } from "./lib/types";

const REPO_KEY = "gitmate.repository";
const RECENTS_KEY = "gitmate.recent";
const MAX_RECENTS = 8;

function readStorage(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function writeStorage(key: string, value: string | null) {
  try {
    if (value === null) localStorage.removeItem(key);
    else localStorage.setItem(key, value);
  } catch {
    // Storage can be unavailable; the app works without remembering.
  }
}

function readRecents(): string[] {
  try {
    const parsed: unknown = JSON.parse(readStorage(RECENTS_KEY) ?? "[]");
    return Array.isArray(parsed) ? parsed.filter((p): p is string => typeof p === "string") : [];
  } catch {
    return [];
  }
}

export type Screen = "overview" | "repository" | "diagnostics" | "profiles" | "rules" | "history" | "settings";

interface AppState {
  info: AppInfo | null;
  infoError: string | null;
  home: string | undefined;
  screen: Screen;
  go: (screen: Screen) => void;
  /** Repository directory the user is looking at. */
  repository: string | null;
  setRepository: (path: string | null) => void;
  recents: string[];
  removeRecent: (path: string) => void;
  clearRecents: () => void;
  /** Incremented after every change so screens reload their data. */
  dataVersion: number;
  refresh: () => void;
}

const AppContext = createContext<AppState | null>(null);

export function AppProvider({ children }: { children: ReactNode }) {
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [infoError, setInfoError] = useState<string | null>(null);
  const [screen, setScreen] = useState<Screen>("overview");
  const [repository, setRepositoryState] = useState<string | null>(() => readStorage(REPO_KEY));
  const [recents, setRecents] = useState<string[]>(readRecents);
  const [dataVersion, setDataVersion] = useState(0);

  useEffect(() => {
    api.appInfo().then(setInfo, (e: unknown) => setInfoError(errorMessage(e)));
  }, []);

  const setRepository = useCallback((path: string | null) => {
    setRepositoryState(path);
    writeStorage(REPO_KEY, path);
    if (path) {
      setRecents((prev) => {
        const next = [path, ...prev.filter((p) => p !== path)].slice(0, MAX_RECENTS);
        writeStorage(RECENTS_KEY, JSON.stringify(next));
        return next;
      });
    }
  }, []);

  const removeRecent = useCallback((path: string) => {
    setRecents((prev) => {
      const next = prev.filter((p) => p !== path);
      writeStorage(RECENTS_KEY, JSON.stringify(next));
      return next;
    });
  }, []);

  const clearRecents = useCallback(() => {
    setRecents([]);
    writeStorage(RECENTS_KEY, null);
  }, []);

  const value = useMemo<AppState>(
    () => ({
      info,
      infoError,
      home: info?.paths.home,
      screen,
      go: setScreen,
      repository,
      setRepository,
      recents,
      removeRecent,
      clearRecents,
      dataVersion,
      refresh: () => setDataVersion((v) => v + 1),
    }),
    [info, infoError, screen, repository, setRepository, recents, removeRecent, clearRecents, dataVersion],
  );

  return <AppContext.Provider value={value}>{children}</AppContext.Provider>;
}

export function useApp(): AppState {
  const ctx = useContext(AppContext);
  if (!ctx) throw new Error("useApp must be used inside AppProvider");
  return ctx;
}

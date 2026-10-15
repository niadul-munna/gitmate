// Typed wrappers around the Rust commands. This is the only module that
// talks to the backend; components never call `invoke` directly.

import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

import type {
  AppInfo,
  ApplyOutcome,
  CommandError,
  DoctorReport,
  OperationRecord,
  PlanTicket,
  Profile,
  ProfileFields,
  ProfileSaved,
  ProfileUsage,
  RepoReport,
  Rule,
} from "./types";

export function isCommandError(value: unknown): value is CommandError {
  return typeof value === "object" && value !== null && "code" in value && "message" in value;
}

/** A readable message for anything a command can throw. */
export function errorMessage(error: unknown): string {
  if (isCommandError(error)) return error.message;
  if (error instanceof Error) return error.message;
  return String(error);
}

export const api = {
  appInfo: () => invoke<AppInfo>("app_info"),

  listProfiles: () => invoke<Profile[]>("list_profiles"),
  getProfile: (id: string) => invoke<ProfileUsage>("get_profile", { id }),
  createProfile: (profile: ProfileFields & { id: string }) =>
    invoke<ProfileSaved>("create_profile", { profile }),
  updateProfile: (id: string, profile: ProfileFields) =>
    invoke<ProfileSaved>("update_profile", { id, profile }),
  deleteProfile: (id: string) => invoke<null>("delete_profile", { id }),

  inspectRepository: (path: string) => invoke<RepoReport>("inspect_repository", { path }),
  runDoctor: (path: string | null) => invoke<DoctorReport>("run_doctor", { path }),
  listRules: () => invoke<Rule[]>("list_rules"),
  listHistory: () => invoke<OperationRecord[]>("list_history"),

  planAssign: (path: string, profile: string, ssh: boolean) =>
    invoke<PlanTicket>("plan_assign", { path, profile, ssh }),
  planAddRule: (directory: string, profile: string) =>
    invoke<PlanTicket>("plan_add_rule", { directory, profile }),
  planRemoveRule: (directory: string) => invoke<PlanTicket>("plan_remove_rule", { directory }),
  planUndo: (operation: string) => invoke<PlanTicket>("plan_undo", { operation }),
  applyPlan: (token: string) => invoke<ApplyOutcome>("apply_plan", { token }),
  discardPlan: (token: string) => invoke<null>("discard_plan", { token }),

  /** Native folder picker. Resolves to null when the user cancels. */
  pickDirectory: async (title: string): Promise<string | null> => {
    const picked = await open({ directory: true, multiple: false, title });
    return typeof picked === "string" ? picked : null;
  },

  /** Native file picker. Only the chosen path is used; the file is not read. */
  pickFile: async (title: string): Promise<string | null> => {
    const picked = await open({ directory: false, multiple: false, title });
    return typeof picked === "string" ? picked : null;
  },
};

export type Api = typeof api;

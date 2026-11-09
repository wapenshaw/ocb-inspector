// Typed bridge to the Rust engine. In a plain browser (design preview) it serves recorded
// CLI output from /mock so layouts can be checked without a Tauri window.
import type {
  BiosSummary,
  Binding,
  BrowsePayload,
  BuildResult,
  CommandError,
  DiffPayload,
  EditPreview,
  InspectPayload,
  Operation,
  Patch,
  Report,
  Selection,
  AppInfo,
  UpdateInfo,
  UiSettings,
  SettingsChange,
  SavedSwitches,
} from "./types";
import { mock, mockPick } from "./mock";

export const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function call<T>(cmd: string, args: Record<string, unknown>): Promise<T> {
  if (inTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<T>(cmd, args);
  }
  return mock<T>(cmd, args);
}

export function errorMessage(e: unknown): string {
  if (e && typeof e === "object" && "message" in e) return String((e as CommandError).message);
  return String(e);
}

export function isCommandError(e: unknown): e is CommandError {
  return !!e && typeof e === "object" && "exit_code" in e;
}

export const api = {
  biosSummary: (bios: string) => call<BiosSummary>("bios_summary", { bios }),
  browse: (bios: string, set = 0) => call<Report<BrowsePayload>>("bios_browse", { bios, set }),
  inspect: (bios: string, profile: string) => call<Report<InspectPayload>>("inspect_pair", { bios, profile }),
  diff: (leftBios: string, leftProfile: string, rightBios: string, rightProfile: string) =>
    call<Report<DiffPayload>>("diff_pairs", { leftBios, leftProfile, rightBios, rightProfile }),
  editValidate: (bios: string, profile: string, base: Binding, operations: Operation[]) =>
    call<EditPreview>("edit_validate", { bios, profile, base, operations }),
  editBuild: (bios: string, profile: string, base: Binding, operations: Operation[], out: string) =>
    call<BuildResult>("edit_build", { bios, profile, base, operations, out }),
  migratePlan: (p: PairPaths, selection: Selection, prior: Patch | null) =>
    call<Patch>("migrate_plan", { ...p, selection, prior }),
  migrateBuild: (p: PairPaths, plan: Patch, out: string) => call<BuildResult>("migrate_build", { ...p, plan, out }),
  savePlan: (plan: Patch, out: string) => call<string>("save_plan", { plan, out }),
  appInfo: () => call<AppInfo>("app_info", {}),
  checkUpdates: () => call<UpdateInfo>("check_updates", {}),
  openPage: (url: string) => call<void>("open_page", { url }),
  settingsLoad: () => call<UiSettings>("settings_load", {}),
  settingsSave: (settings: SettingsChange) => call<SavedSwitches>("settings_save", { settings }),
};

export interface PairPaths {
  sourceBios: string;
  sourceProfile: string;
  targetBios: string;
  targetProfile: string;
}

export async function pickFile(kind: "bios" | "profile"): Promise<string | null> {
  if (!inTauri) return mockPick(kind);
  const { open } = await import("@tauri-apps/plugin-dialog");
  const filters =
    kind === "profile"
      ? [{ name: "OC profile", extensions: ["ocb"] }]
      : [{ name: "BIOS image", extensions: ["*"] }];
  const r = await open({ multiple: false, directory: false, filters, title: kind === "bios" ? "Open a BIOS image" : "Open an OC profile" });
  return typeof r === "string" ? r : null;
}

export async function pickSave(defaultPath: string, kind: "ocb" | "json"): Promise<string | null> {
  if (!inTauri) return `preview/${defaultPath}`;
  const { save } = await import("@tauri-apps/plugin-dialog");
  const filters = kind === "ocb" ? [{ name: "OC profile", extensions: ["ocb"] }] : [{ name: "JSON", extensions: ["json"] }];
  return save({ defaultPath, filters, title: kind === "ocb" ? "Export profile" : "Save migration plan" });
}

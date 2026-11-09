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
  VerifyPayload,
  AppInfo,
  UpdateInfo,
} from "./types";

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
  verify: (bios: string, profile: string) => call<Report<VerifyPayload>>("verify_pair", { bios, profile }),
  diff: (leftBios: string, leftProfile: string, rightBios: string, rightProfile: string) =>
    call<Report<DiffPayload>>("diff_pairs", { leftBios, leftProfile, rightBios, rightProfile }),
  editValidate: (bios: string, profile: string, base: Binding, operations: Operation[]) =>
    call<EditPreview>("edit_validate", { bios, profile, base, operations }),
  editBuild: (bios: string, profile: string, base: Binding, operations: Operation[], out: string) =>
    call<BuildResult>("edit_build", { bios, profile, base, operations, out }),
  migratePlan: (p: PairPaths, selection: Selection, prior: Patch | null) =>
    call<Patch>("migrate_plan", { ...pairArgs(p), selection, prior }),
  migrateBuild: (p: PairPaths, plan: Patch, out: string) => call<BuildResult>("migrate_build", { ...pairArgs(p), plan, out }),
  savePlan: (plan: Patch, out: string) => call<string>("save_plan", { plan, out }),
  appInfo: () => call<AppInfo>("app_info", {}),
  checkUpdates: () => call<UpdateInfo>("check_updates", {}),
  openPage: (url: string) => call<void>("open_page", { url }),
};

export interface PairPaths {
  sourceBios: string;
  sourceProfile: string;
  targetBios: string;
  targetProfile: string;
}

function pairArgs(p: PairPaths) {
  return {
    sourceBios: p.sourceBios,
    sourceProfile: p.sourceProfile,
    targetBios: p.targetBios,
    targetProfile: p.targetProfile,
  };
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

// ---------------------------------------------------------------- design preview only

const mockQueue: Record<string, string[]> = {
  bios: ["E7D91IMS.HI0", "E7D91IMS.HJ0", "E7E49AMSI.1AA7", "E7C02AMS.HG4"],
  profile: ["BeforeUpgrade.ocb", "Base-100mv-NoOC.ocb"],
};

function mockPick(kind: "bios" | "profile"): string {
  const q = mockQueue[kind];
  const v = q.shift() ?? q[q.length - 1] ?? "";
  q.push(v);
  return v;
}

async function mock<T>(cmd: string, args: Record<string, unknown>): Promise<T> {
  await new Promise((r) => setTimeout(r, 150));
  const file = (n: string) => String(n).split(/[\\/]/).pop() ?? "";
  let name: string;
  switch (cmd) {
    case "bios_summary":
      name = `bios-${file(args.bios as string)}`;
      break;
    case "bios_browse":
      name = `browse-${file(args.bios as string)}${args.set ? `-set${(args.set as number) + 1}` : ""}`;
      break;
    case "inspect_pair":
    case "verify_pair": {
      const b = file(args.bios as string);
      const p = file(args.profile as string);
      // Preview fixtures: BeforeUpgrade was saved by HI0, the others by HJ0.
      if (b.endsWith("HI0") !== p.startsWith("Before")) {
        throw { code: "identity_mismatch", exit_code: 4, message: `BIOS/profile identity mismatch: profile was not saved by ${b}` };
      }
      name = `${cmd === "inspect_pair" ? "inspect" : "verify"}-${p}`;
      break;
    }
    case "diff_pairs":
      // Preview fixtures: two HJ0 profiles, or HI0 against HJ0.
      name = String(args.leftBios).endsWith("HJ0") && String(args.rightBios).endsWith("HJ0") ? "diff-same" : "diff-cross";
      break;
    case "migrate_plan":
      name = String(args.sourceBios).endsWith("HJ0") ? "plan-down" : "plan";
      break;
    case "app_info":
      return {
        version: "0.1.0",
        changelog: `# Changelog

## 0.1.0 (2026-11-09)


### Features

* initial release of OCB Inspector ([ce6c7f9](https://github.com/wapenshaw/ocb-inspector/commit/ce6c7f9))
`,
        repo_url: "https://github.com/wapenshaw/ocb-inspector",
        update_check: true,
      } as T;
    case "check_updates":
      // Preview: ?update in the address shows a newer release.
      return {
        current: "0.1.0",
        newer: location.search.includes("update")
          ? [{ version: "0.2.0", url: "https://github.com/wapenshaw/ocb-inspector/releases/tag/v0.2.0", published_at: "2026-11-20T10:00:00Z",
               notes: `## [0.2.0](https://github.com/wapenshaw/ocb-inspector/compare/v0.1.0...v0.2.0) (2026-11-20)


### Features

* **desktop:** show the app version and check for updates ([abc1234](https://github.com/wapenshaw/ocb-inspector/commit/abc1234))


### Bug Fixes

* **ui:** keep the menu rail from shifting ([def5678](https://github.com/wapenshaw/ocb-inspector/commit/def5678))
` }]
          : [],
      } as T;
    case "open_page":
      window.open(String(args.url), "_blank", "noopener");
      return undefined as T;
    case "migrate_build":
      return { output_path: args.out, report_path: String(args.out).replace(/\.ocb$/, ".manifest.json"), manifest: await fetchJson("manifest") } as T;
    default:
      throw { code: "unsupported", exit_code: 5, message: `${cmd} is not available in the design preview` };
  }
  return fetchJson<T>(name);
}

async function fetchJson<T>(name: string): Promise<T> {
  const r = await fetch(`/mock/${name}.json`);
  if (!r.ok) throw { code: "io", exit_code: 3, message: `preview data ${name} missing` };
  return r.json();
}

import type { SavedSwitches, SettingsChange, UiSettings } from "./types";
import { fileName } from "../lib/format";

// Design preview only: outside Tauri the bridge serves recorded CLI output from /mock
// (ui/public/mock, git-ignored) so layouts can be checked without a desktop window.
const mockQueue: Record<"bios" | "profile", string[]> = {
  bios: ["E7D91IMS.HI0", "E7D91IMS.HJ0", "E7E49AMSI.1AA7", "E7C02AMS.HG4", "E7C77IMS.1C0", "E7E02IMS.1C0"],
  profile: ["BeforeUpgrade.ocb", "Base-100mv-NoOC.ocb", "E7C77IMS.1C0.ocb", "E7E02IMS.1C0.ocb"],
};

export function mockPick(kind: "bios" | "profile"): string {
  const q = mockQueue[kind];
  const v = q.shift() ?? q[q.length - 1] ?? "";
  q.push(v);
  return v;
}

// Settings live in localStorage, standing in for config/settings.json. "ocb.writing" = "1" shows
// the experimental writing UI.
function previewSettings(cmd: string, change: SettingsChange): UiSettings | SavedSwitches {
  const keys = { theme: "ocb.theme", update_check: "ocb.update_check", writing: "ocb.writing" } as const;
  if (cmd === "settings_save") {
    if (change.theme !== undefined) localStorage.setItem(keys.theme, change.theme);
    for (const k of ["update_check", "writing"] as const) {
      if (change[k] !== undefined) localStorage.setItem(keys[k], change[k] ? "1" : "0");
    }
  }
  const saved = { update_check: localStorage.getItem(keys.update_check) !== "0", writing: localStorage.getItem(keys.writing) === "1" };
  if (cmd === "settings_save") return saved;
  return {
    theme: localStorage.getItem(keys.theme),
    writing: saved.writing,
    writing_note: "Preview: set localStorage ocb.writing = 1 to show writing.",
    path: "config/settings.json (browser preview: localStorage)",
    saved,
    error: "",
  };
}

export async function mock<T>(cmd: string, args: Record<string, unknown>): Promise<T> {
  if (cmd === "settings_load" || cmd === "settings_save") return previewSettings(cmd, (args.settings ?? {}) as SettingsChange) as T;
  await new Promise((r) => setTimeout(r, 150));
  const file = fileName;
  let name: string;
  switch (cmd) {
    case "bios_summary":
      name = `bios-${file(args.bios as string)}`;
      break;
    case "bios_browse":
      name = `browse-${file(args.bios as string)}${args.set ? `-set${(args.set as number) + 1}` : ""}`;
      break;
    case "inspect_pair": {
      const b = file(args.bios as string);
      const p = file(args.profile as string);
      // Preview fixtures: BeforeUpgrade was saved by HI0, the others by HJ0.
      if (b.endsWith("HI0") !== p.startsWith("Before")) {
        throw { code: "identity_mismatch", exit_code: 4, message: `BIOS/profile identity mismatch: profile was not saved by ${b}` };
      }
      name = `inspect-${p}`;
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
    case "analyze_profile":
      return analyzeMock<T>(args);
    case "profile_format":
      return profileFormatMock<T>(args);
    case "save_profile_format":
    case "save_analysis":
      // The preview has no file system: the chosen path is the result.
      return String(args.out) as T;
    case "migrate_build":
      return { output_path: args.out, report_path: String(args.out).replace(/\.ocb$/, ".manifest.json"), manifest: await fetchJson("manifest") } as T;
    default:
      throw { code: "unsupported", exit_code: 5, message: `${cmd} is not available in the design preview` };
  }
  return fetchJson<T>(name);
}

// The recorded analyses are git-ignored; without them the preview serves the synthetic fixture
// (loaded on demand, so it stays out of the main bundle).
async function analyzeMock<T>(args: Record<string, unknown>): Promise<T> {
  const profile = fileName(args.profile as string);
  const bios = args.bios ? fileName(args.bios as string) : "";
  const base = `/mock/analyze-${profile}${bios ? `-${bios}` : ""}`;
  // A recording made with --baseline carries a -baseline-<name> suffix, and with --baseline-bios an
  // -on-<bios> suffix after that; without one the plain recording is used.
  const baseName = bios && args.baseline ? `${base}-baseline-${fileName(args.baseline as string)}` : "";
  const names = baseName
    ? args.baselineBios
      ? [`${baseName}-on-${fileName(args.baselineBios as string)}`, baseName, base]
      : [baseName, base]
    : [base];
  for (const n of names) {
    const r = await fetch(`${n}.json`);
    // The dev server answers a missing file with its HTML page, so check the type as well.
    if (r.ok && r.headers.get("content-type")?.includes("json")) return (await r.json()) as T;
  }
  const { analysisFixture } = await import("../test/analysisFixture");
  // A baseline is accepted; the synthetic report annotates against one the same way the engine does.
  return analysisFixture({
    bios: !!bios,
    set: (args.set as number | null) ?? null,
    baseline: bios && args.baseline ? "applied" : undefined,
    cross: !!(bios && args.baseline && args.baselineBios),
  }) as T;
}

async function profileFormatMock<T>(args: Record<string, unknown>): Promise<T> {
  const bios = fileName(args.bios as string);
  const set = (args.set as number | null) ?? 0;
  const r = await fetch(`/mock/format-${bios}${set ? `-set${set + 1}` : ""}.json`);
  if (r.ok && r.headers.get("content-type")?.includes("json")) return (await r.json()) as T;
  const { profileFormatFixture } = await import("../test/profileFormatFixture");
  return profileFormatFixture() as T;
}

async function fetchJson<T>(name: string): Promise<T> {
  const r = await fetch(`/mock/${name}.json`);
  if (!r.ok) throw { code: "io", exit_code: 3, message: `preview data ${name} missing` };
  return r.json();
}

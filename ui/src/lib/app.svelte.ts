// App-wide state read once at start-up from config/settings.json (see lib/theme.ts).
// Writing is experimental and off unless the settings file turns it on; changing it needs a restart.
export const app = $state({
  /** Edit and Port to another BIOS are available. */
  writing: false,
  /** Why writing is off (shown on hover in read-only mode). */
  writingNote: "",
});

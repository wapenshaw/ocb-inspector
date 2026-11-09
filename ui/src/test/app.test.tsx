import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { App } from "../App";
import { aboutStore } from "../lib/updates";

const release = {
  version: "0.3.0",
  notes: "## 0.3.0 (2026-12-01)\n\n### Features\n\n* something new",
  url: "https://github.com/wapenshaw/ocb-inspector/releases/tag/v0.3.0",
  published_at: "2026-12-01T10:00:00Z",
};
const info = { version: "0.2.0", changelog: "## 0.2.0 (2026-11-09)\n", repo_url: "https://github.com/wapenshaw/ocb-inspector", update_check: true };

beforeEach(() => {
  aboutStore.set({ info, state: "current", newer: [], error: "", checkedAt: null });
  document.documentElement.dataset.theme = "light";
});

describe("header", () => {
  it("shows the version alone when up to date, and a badge when a newer release exists", () => {
    render(<App />);
    const button = screen.getByRole("button", { name: /v0\.2\.0/ });
    expect(button).toHaveTextContent(/^v0\.2\.0$/);
    act(() => aboutStore.set({ state: "available", newer: [release] }));
    expect(button).toHaveTextContent("Update");
    expect(button).toHaveAttribute("title", "Version 0.3.0 is available");
  });

  it("opens About with the newer release's notes", async () => {
    aboutStore.set({ state: "available", newer: [release] });
    render(<App />);
    await userEvent.click(screen.getByRole("button", { name: /v0\.2\.0/ }));
    expect(screen.getByText("Version 0.3.0 is available")).toBeInTheDocument();
    expect(screen.getByText("something new")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Open the v0.3.0 release" })).toBeInTheDocument();
  });

  it("switches the colour mode", async () => {
    render(<App />);
    await userEvent.click(screen.getByRole("radio", { name: "Dark" }));
    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(screen.getByRole("radio", { name: "Dark" })).toHaveAttribute("aria-checked", "true");
  });
});

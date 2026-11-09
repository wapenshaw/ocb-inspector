import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Settings } from "../components/Settings";
import { appStore } from "../state/app";

// Outside Tauri the switches are kept in localStorage, standing in for config/settings.json.
beforeEach(() => {
  localStorage.clear();
  appStore.set({ writing: false, saved: { update_check: true, writing: false }, settingsError: "", settingsPath: "" });
});

describe("Settings", () => {
  it("asks before turning writing on, then says a restart is needed", async () => {
    render(<Settings />);
    await userEvent.click(screen.getByRole("checkbox", { name: /Experimental writing/ }));
    expect(screen.getByRole("alertdialog")).toHaveTextContent("only loading it on your motherboard");
    expect(localStorage.getItem("ocb.writing")).toBeNull();

    await userEvent.click(screen.getByRole("button", { name: "Turn on writing" }));
    expect(localStorage.getItem("ocb.writing")).toBe("1");
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
    expect(screen.getByText("Restart OCB Inspector to turn writing on")).toBeInTheDocument();
  });

  it("cancelling leaves writing off", async () => {
    render(<Settings />);
    await userEvent.click(screen.getByRole("checkbox", { name: /Experimental writing/ }));
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.getByRole("checkbox", { name: /Experimental writing/ })).not.toBeChecked();
    expect(localStorage.getItem("ocb.writing")).toBeNull();
  });

  it("saves the update check switch", async () => {
    render(<Settings />);
    const box = screen.getByRole("checkbox", { name: /Check for updates/ });
    expect(box).toBeChecked();
    await userEvent.click(box);
    expect(box).not.toBeChecked();
    expect(localStorage.getItem("ocb.update_check")).toBe("0");
  });

  it("disables the switches when the settings file cannot be used", () => {
    appStore.set({ settingsError: "config/settings.json is not valid JSON" });
    render(<Settings />);
    expect(screen.getByRole("checkbox", { name: /Check for updates/ })).toBeDisabled();
    expect(screen.getByText("config/settings.json is not valid JSON")).toBeInTheDocument();
  });
});

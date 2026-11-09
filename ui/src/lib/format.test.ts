import { cpuShort, display, fileName, hex, sameIntent, savedAt, shortHash } from "./format";

describe("format", () => {
  it("shows values with units, Auto as a tag and unknown raws plainly", () => {
    expect(display({ kind: "auto" })).toBe("Auto");
    expect(display({ kind: "integer", value: 105, unit: "mV" })).toBe("105 mV");
    expect(display({ kind: "integer", value: 2, unit: "ratio" })).toBe("+2 bins");
    expect(display({ kind: "boolean", value: true })).toBe("On");
    expect(display({ kind: "unknown", raw: 7 })).toBe("7 unknown raw value");
    expect(display(null)).toBe("—");
  });

  it("compares enums by symbol", () => {
    expect(sameIntent({ kind: "enum", symbol: "on", label: "Enabled" }, { kind: "enum", symbol: "on", label: "On" })).toBe(true);
    expect(sameIntent({ kind: "auto" }, null)).toBe(false);
  });

  it("formats save times, hashes, paths, CPU names and offsets", () => {
    expect(savedAt("20261108111900")).toBe("8 Nov 2026, 11:19");
    expect(savedAt("bad")).toBe("bad");
    expect(shortHash("0123456789abcdef")).toBe("01234567…cdef");
    expect(fileName("C:\\bios\\E7D91IMS.HJ0")).toBe("E7D91IMS.HJ0");
    expect(fileName("/home/me/E7D91IMS.HJ0")).toBe("E7D91IMS.HJ0");
    expect(cpuShort("Intel(R) Core(TM)  i9")).toBe("Intel Core i9");
    expect(hex(0xcd6)).toBe("0x0CD6");
  });
});

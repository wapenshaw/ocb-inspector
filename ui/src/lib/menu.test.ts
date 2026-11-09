import { menuPages, menuSort, pageBefore, sectionBefore } from "./menu";

const item = (path: string[], order: number[], section: string | null = null) => ({ menu: { path, section, order } });

describe("menu", () => {
  const items = [
    item(["OC", "Voltage"], [2, 1], "Core"),
    item(["OC"], [1, 0]),
    item(["OC", "Voltage"], [2, 0], "Core"),
    item(["Advanced", "CPU"], [0, 5]),
    { menu: null },
  ];

  it("sorts pages in menu order, then by position, with unplaced items last", () => {
    const sorted = menuSort(items);
    expect(sorted.map((s) => s.menu?.order ?? null)).toEqual([[0, 5], [1, 0], [2, 0], [2, 1], null]);
  });

  it("adds missing ancestors as headings", () => {
    expect(menuPages(items).map((p) => [p.key, p.depth, p.heading])).toEqual([
      ["Advanced", 0, true],
      ["Advanced › CPU", 1, false],
      ["OC", 0, false],
      ["OC › Voltage", 1, false],
    ]);
  });

  it("prints page and section headings only where they change", () => {
    const sorted = menuSort(items);
    expect(sorted.map((_, i) => pageBefore(sorted, i))).toEqual([["Advanced", "CPU"], ["OC"], ["OC", "Voltage"], null, []]);
    expect(sorted.map((_, i) => sectionBefore(sorted, i))).toEqual([null, null, "Core", null, null]);
  });
});

import { parseChangelog } from "./updates";

const md = `# Changelog

## [0.2.0](https://github.com/wapenshaw/ocb-inspector/compare/v0.1.0...v0.2.0) (2026-11-20)


### Features

* **desktop:** show the version and check for updates ([beb7e72](https://github.com/wapenshaw/ocb-inspector/commit/beb7e72)), closes [#5](https://github.com/wapenshaw/ocb-inspector/issues/5)


### Bug Fixes

* keep the \`menu\` rail from _shifting_ ([def5678](https://github.com/wapenshaw/ocb-inspector/commit/def5678))

## 0.1.0 (2026-11-09)


### Features

* initial release of OCB Inspector ([ce6c7f9](https://github.com/wapenshaw/ocb-inspector/commit/ce6c7f9))
`;

describe("parseChangelog", () => {
  it("reads versions, dates, sections and items, dropping links and hashes", () => {
    expect(parseChangelog(md)).toEqual([
      {
        version: "0.2.0",
        date: "2026-11-20",
        sections: [
          { title: "Features", items: ["desktop: show the version and check for updates"] },
          { title: "Bug Fixes", items: ["keep the menu rail from shifting"] },
        ],
      },
      { version: "0.1.0", date: "2026-11-09", sections: [{ title: "Features", items: ["initial release of OCB Inspector"] }] },
    ]);
  });

  it("returns nothing for text without versions", () => {
    expect(parseChangelog("just notes\n* an item")).toEqual([]);
  });
});

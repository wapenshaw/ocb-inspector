# Changelog

## [0.4.0](https://github.com/wapenshaw/ocb-inspector/compare/v0.3.0...v0.4.0) (2026-11-10)


### Features

* analyze any profile experimentally, with baseline comparison ([4efb4d2](https://github.com/wapenshaw/ocb-inspector/commit/4efb4d2525e03b764ae66ba61040701d19e8fd1f))


### Bug Fixes

* **core:** read profiles whose CMOS block contains the $OCI$ bytes ([02e5bc0](https://github.com/wapenshaw/ocb-inspector/commit/02e5bc013dd50caae43d083044b07a29536d3a10))
* **desktop:** do not offer the experimental analysis on a BIOS mismatch ([d7af08a](https://github.com/wapenshaw/ocb-inspector/commit/d7af08a2a00ed65d1d88d762533851915a81c7f9))

## [0.3.0](https://github.com/wapenshaw/ocb-inspector/compare/v0.2.0...v0.3.0) (2026-11-09)


### Features

* **desktop:** move the UI to React and add a Settings card ([3a9e326](https://github.com/wapenshaw/ocb-inspector/commit/3a9e326f49fa0d99095d63d3552fe38a76cd1a46))


### Bug Fixes

* **core:** enforce the read limit while reading, not only before ([f8cfb34](https://github.com/wapenshaw/ocb-inspector/commit/f8cfb341f3bdf5687add13dc57afb5944944761c))
* **core:** stop the firmware walk at file sizes that overflow ([6faa585](https://github.com/wapenshaw/ocb-inspector/commit/6faa5853cbeda6aa8b3362bdf64a8ef507a77488))
* **ui:** keep every choice when Port re-plans quickly, and drop late answers ([3a9e326](https://github.com/wapenshaw/ocb-inspector/commit/3a9e326f49fa0d99095d63d3552fe38a76cd1a46))
* **ui:** keep Port choices when a re-plan fails, and offer to try again ([3a9e326](https://github.com/wapenshaw/ocb-inspector/commit/3a9e326f49fa0d99095d63d3552fe38a76cd1a46))
* **ui:** never show a Compare result for files that changed meanwhile ([3a9e326](https://github.com/wapenshaw/ocb-inspector/commit/3a9e326f49fa0d99095d63d3552fe38a76cd1a46))

## [0.2.0](https://github.com/wapenshaw/ocb-inspector/compare/v0.1.0...v0.2.0) (2026-11-09)


### Features

* **desktop:** show the version and check for updates ([beb7e72](https://github.com/wapenshaw/ocb-inspector/commit/beb7e7280e169b98787ca49e2fc1ef09935b5aa0))

## 0.1.0 (2026-11-09)


### Features

* initial release of OCB Inspector ([ce6c7f9](https://github.com/wapenshaw/ocb-inspector/commit/ce6c7f9d5575a91e8e31df00434ae1c0e7c8ae76))

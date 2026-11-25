# Changelog

All notable changes to this project will be documented in this file.

## [0.8.0](https://github.com/nominal-io/nominal-client-rs/compare/nominal-v0.7.2...nominal-v0.8.0) - 2026-11-24

### Added

- [**breaking**] upgrade nominal-api to 0.1519.0 and nominal-streaming to 0.11.0 ([#217](https://github.com/nominal-io/nominal-client-rs/pull/217))
- add installer scripts ([#183](https://github.com/nominal-io/nominal-client-rs/pull/183))

### Other

- *(deps)* bump nominal-api from 0.1519.0 to 0.1546.0 ([#233](https://github.com/nominal-io/nominal-client-rs/pull/233))
- *(deps)* bump prost from 0.13.5 to 0.14.3 ([#16](https://github.com/nominal-io/nominal-client-rs/pull/16))
- *(deps)* bump dirs from 5.0.1 to 6.0.0 ([#24](https://github.com/nominal-io/nominal-client-rs/pull/24))
- generate page for dev.nom.io ([#219](https://github.com/nominal-io/nominal-client-rs/pull/219))

## [0.8.0](https://github.com/nominal-io/nominal-client-rs/compare/nominal-v0.7.2...nominal-v0.8.0) - 2026-11-24

### Added

- [**breaking**] upgrade nominal-api to 0.1519.0 and nominal-streaming to 0.11.0 ([#217](https://github.com/nominal-io/nominal-client-rs/pull/217))

### Fixed

- recover missing multipart ETags without repeating successful uploads ([#234](https://github.com/nominal-io/nominal-client-rs/pull/234))
- serialize build for docs.rs OOM kill ([#196](https://github.com/nominal-io/nominal-client-rs/pull/196))

### Other

- *(deps)* bump the external-deps group with 6 updates ([#236](https://github.com/nominal-io/nominal-client-rs/pull/236))
- *(deps)* bump actions-rust-lang/setup-rust-toolchain from 1.16.1 to 2.0.1 ([#212](https://github.com/nominal-io/nominal-client-rs/pull/212))
- *(deps)* bump nominal-api from 0.1519.0 to 0.1546.0 ([#233](https://github.com/nominal-io/nominal-client-rs/pull/233))
- *(deps)* bump dirs from 5.0.1 to 6.0.0 ([#24](https://github.com/nominal-io/nominal-client-rs/pull/24))
- *(deps)* bump webpki-roots and webpki-root-certs to 1.0.11 ([#210](https://github.com/nominal-io/nominal-client-rs/pull/210))

## [0.7.2](https://github.com/nominal-io/nominal-client-rs/compare/nominal-v0.7.1...nominal-v0.7.2) - 2026-09-18

### Added

- add checklist client ([#172](https://github.com/nominal-io/nominal-client-rs/pull/172))
- align grpc retries with python client ([#154](https://github.com/nominal-io/nominal-client-rs/pull/154))

## [0.7.1](https://github.com/nominal-io/nominal-client-rs/compare/nominal-v0.7.0...nominal-v0.7.1) - 2026-08-20

### Fixed

- decode grpc error details ([#146](https://github.com/nominal-io/nominal-client-rs/pull/146))
- drive file listing with prefix and virtual drives ([#145](https://github.com/nominal-io/nominal-client-rs/pull/145))

## [0.7.0](https://github.com/nominal-io/nominal-client-rs/compare/nominal-v0.6.0...nominal-v0.7.0) - 2026-08-15

### Added

- *(drives)* add file download, remove drives ff ([#144](https://github.com/nominal-io/nominal-client-rs/pull/144))

## [0.6.0](https://github.com/nominal-io/nominal-client-rs/compare/nominal-v0.5.3...nominal-v0.6.0) - 2026-07-31

### Added

- add unstable `nomctl fs` subcommand ([#102](https://github.com/nominal-io/nominal-client-rs/pull/102))

## [0.5.3](https://github.com/nominal-io/nominal-client-rs/compare/nominal-v0.5.2...nominal-v0.5.3) - 2026-07-17

### Fixed

- scope asset, dataset, video, run, and workbook searches by the client's workspace_rid ([#128](https://github.com/nominal-io/nominal-client-rs/pull/128))

## [0.5.2](https://github.com/nominal-io/nominal-client-rs/compare/nominal-v0.5.1...nominal-v0.5.2) - 2026-07-01

### Other

- add workspace list selector and updated validation states ([#112](https://github.com/nominal-io/nominal-client-rs/pull/112))

## [0.5.1](https://github.com/nominal-io/nominal-client-rs/compare/nominal-v0.5.0...nominal-v0.5.1) - 2026-06-15

### Added

- add archive methods to workbook client ([#100](https://github.com/nominal-io/nominal-client-rs/pull/100))

## [0.5.0](https://github.com/nominal-io/nominal-client-rs/compare/nominal-v0.4.3...nominal-v0.5.0) - 2026-06-12

### Added

- add workbook and template clients ([#96](https://github.com/nominal-io/nominal-client-rs/pull/96))
- harden config and profile management ([#80](https://github.com/nominal-io/nominal-client-rs/pull/80))

## [0.4.3](https://github.com/nominal-io/nominal-client-rs/compare/nominal-v0.4.2...nominal-v0.4.3) - 2026-05-21

### Added

- add channel delimiter in dataset create ([#70](https://github.com/nominal-io/nominal-client-rs/pull/70))

### Fixed

- nominal_url method returns a valid url ([#72](https://github.com/nominal-io/nominal-client-rs/pull/72))

## [0.4.2](https://github.com/nominal-io/nominal-client-rs/compare/nominal-v0.4.1...nominal-v0.4.2) - 2026-05-20

### Added

- add dataset with tag filter to an asset ([#67](https://github.com/nominal-io/nominal-client-rs/pull/67))

## [0.4.1](https://github.com/nominal-io/nominal-client-rs/compare/nominal-v0.4.0...nominal-v0.4.1) - 2026-05-18

### Added

- add & re-export nominal-streaming ([#63](https://github.com/nominal-io/nominal-client-rs/pull/63))

## [0.4.0](https://github.com/nominal-io/nominal-client-rs/compare/nominal-v0.3.0...nominal-v0.4.0) - 2026-05-18

### Added

- run create/search, asset create/search ([#38](https://github.com/nominal-io/nominal-client-rs/pull/38))

### Fixed

- use series metadata rather than channel metadata service for upsert channel info ([#60](https://github.com/nominal-io/nominal-client-rs/pull/60))

## [0.3.0](https://github.com/nominal-io/nominal-client-rs/compare/nominal-v0.2.3...nominal-v0.3.0) - 2026-04-29

### Added

- client builder, user-agent ([#32](https://github.com/nominal-io/nominal-client-rs/pull/32))

## [0.2.3](https://github.com/nominal-io/nominal-client-rs/compare/nominal-v0.2.2...nominal-v0.2.3) - 2026-04-28

### Other

- cli release updates ([#29](https://github.com/nominal-io/nominal-client-rs/pull/29))

## [0.2.2](https://github.com/nominal-io/nominal-client-rs/compare/nominal-v0.2.1...nominal-v0.2.2) - 2026-04-28

### Other

- update nominal-api ([#25](https://github.com/nominal-io/nominal-client-rs/pull/25))

## [0.2.1] - 2026-04-26

### Fixed

- Published a clean baseline for release-plz after the original 0.2.0 crate metadata pointed at an unpackageable workspace commit.

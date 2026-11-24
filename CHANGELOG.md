# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.0.2] - 2026-11-24

### Changed

- Refreshed compatible transitive dependencies while keeping the Rust 1.88 minimum and existing direct dependency versions.

## [1.0.1] - 2026-08-18

### Changed

- Updated Chrono to 0.4.45, Ratatui to 0.30.2, and transitive dependencies.
- Raised the minimum supported Rust version to 1.88 to match Ratatui's requirements.

### Fixed

- Improved terminal redraw behavior after wide styled cells through the Ratatui update.

[Unreleased]: https://github.com/Feriixu/worldclock-tui-rs/compare/v1.0.2...HEAD
[1.0.2]: https://github.com/Feriixu/worldclock-tui-rs/compare/v1.0.1...v1.0.2
[1.0.1]: https://github.com/Feriixu/worldclock-tui-rs/releases/tag/v1.0.1

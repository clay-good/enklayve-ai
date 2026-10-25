# Cross-Platform Release

## Purpose

Enklayve ships as installable desktop apps for every major OS, built and
published automatically by CI. Users download one file and run it; there is no
build step, no dependency install, no mobile app.

## Requirements

### Requirement: CI SHALL build and publish installers for all desktop targets on tag push

A version tag SHALL trigger GitHub Actions to build and publish installers for
macOS (Apple Silicon and Intel), Windows, and Linux.

#### Scenario: Tagged release
- **WHEN** a `v*` tag is pushed
- **THEN** CI produces a DMG (arm64 + x86_64), a Windows installer, and a Linux
  AppImage, and attaches them to the GitHub Release

#### Scenario: One target fails
- **WHEN** one platform's build fails
- **THEN** the others still publish (the matrix does not fail-fast), and the
  failure is clearly reported

### Requirement: Installers SHALL run without a developer toolchain or manual setup

A downloaded installer SHALL run on a clean machine with no Node, Rust, Docker,
or command-line steps required by the user.

#### Scenario: Clean machine install
- **WHEN** a user installs the app on a machine with no developer tools
- **THEN** the app launches and reaches onboarding with no extra steps
  (barring documented OS gatekeeper prompts for unsigned builds)

### Requirement: The app SHALL target desktop only

No mobile (iOS/Android) build SHALL be produced or maintained.

#### Scenario: Scope check
- **WHEN** release artifacts are produced
- **THEN** they are desktop installers only

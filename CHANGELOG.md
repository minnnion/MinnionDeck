# Changelog

Every MinnionDeck release, newest first. Downloads are on the [Releases](https://github.com/minnnion/MinnionDeck/releases) page.
The format follows [Keep a Changelog](https://keepachangelog.com/), and versions follow [Semantic Versioning](https://semver.org/).

## [0.2.0] - 2026-11-10

### Added
- **Secure phone link** (Settings › Phones): the QR code opens the HTTPS page.
- **Live chat on a button**, drawn from a plugin, with scroll back.
- Buttons up to **3 × 3** cells (was 2 × 2).
- **Choose pages** when adding a plugin profile that has one page per service; profile updates keep your choice.
- Plugin pictures on Marketplace cards.

### Changed
- Your data's key is **locked to your Windows account** (DPAPI). Moving to another PC is done by exporting profiles.
- Updates must be **signed** with MinnionDeck's key (`.sig` beside the installer).
- The app is about 1 MB smaller.
- Plugin installs count what really unpacks and refuse links inside the file.

### Fixed
- Plugin windows with many settings couldn't scroll to Save.
- Dropdowns that depend on another field (OBS Source after Scene) didn't update.
- Marketplace plugin buttons (like Discord) showed a lightning bolt instead of their pictures.

### Security
- zip library updated for RUSTSEC-2026-0367, -0368, -0369 and -0429.
- The phone page serves at most 64 connections at once.
- The editor link's key is checked in constant time.
- CI builds, tests and audits the dependencies on every push and weekly.

## [0.1.2] - 2026-11-07

### Added
- The **image editor and GIF maker inside MinnionDeck**, served from your own PC, with layers and **Sync** back to the button.
- **Edit image** in the Edit button window, using an already open editor tab when there is one.
- Plugin profiles **update with their plugin** (with a backup; a profile you changed asks first).
- A tray note the first time the window is closed.

### Changed
- **Make a picture** and **Make a GIF** replace *Make on the website*.

### Fixed
- Edit image on a button without a picture, GIFs opening as GIFs, the icon drawn twice after Sync, and a second profile install adding a copy.

## [0.1.1] - 2026-11-07

### Fixed
- Plugin buttons from an imported profile no longer ask to be allowed; only buttons that act on the PC still ask.
- Buttons move only in **Edit layout**.
- No small dots on buttons with more than one look.
- Settings › About shows the real version.

## [0.1.0] - 2026-11-06

First release of MinnionDeck, the native Rust desktop app: profiles, pages and buttons, phones over Wi-Fi, the Marketplace, tray, backups and updates.

[0.2.0]: https://github.com/minnnion/MinnionDeck/compare/v0.1.2...v0.2.0
[0.1.2]: https://github.com/minnnion/MinnionDeck/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/minnnion/MinnionDeck/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/minnnion/MinnionDeck/releases/tag/v0.1.0

# Changelog

Every MinnionDeck release, newest first. Downloads are on the [Releases](https://github.com/minnnion/MinnionDeck/releases) page.
The format follows [Keep a Changelog](https://keepachangelog.com/), and versions follow [Semantic Versioning](https://semver.org/).

## [0.2.2] - 2026-11-13

### Added
- **One-time phone setup, no certificate warning:** the PC makes its own certificate authority for the phone page, limited to home-network addresses. The QR code opens a setup page that checks whether the phone trusts it and, if not, shows how to install it once; then the deck opens over HTTPS with no warning, also from the Home Screen and after the PC's address changes.
- **Password backups** (Settings › Backup › Move to another PC): a backup locked with a password you choose, which opens on another PC or after Windows is installed again. Ordinary backups still open only on the Windows account that made them.

### Changed
- **Phones connect over HTTPS by default.** The QR code opens the HTTPS page; the phone warns about the certificate once. Phones you paired before keep working: "Allow the plain http page" (Settings › Phones) is switched on for installs that already had phones paired.
- "Allow imported buttons?" now lists what the buttons will do.
- The plugin install question shows the signing key's fingerprint and warns before putting an older version over a newer one. Turning on Developer mode asks first.
- The installer's helpers hand their work to MinnionDeck when it is running.

### Fixed
- A library or settings file that another program had open for a moment could be set aside, and an empty library saved in its place. It is now waited for, and never saved over.
- A restore that stopped half-way (the PC went off) could leave pictures and buttons that didn't match. It is now finished or undone at the next start, and old pictures are kept until it is complete.
- A profile file with one damaged picture comes in without it, instead of not at all.
- A key combination that Windows only half sent could leave Ctrl or Shift held down.
- Pictures that nothing uses any more are cleared out, and looking at a profile file no longer copies its pictures.
- The window no longer wakes up twice a second when nothing changes, the phone deck is rebuilt only when it changed, and backups no longer pause the window.

### Security
- A plugin author's key file is trusted only after you confirm it, showing the name and the key's fingerprint.
- An update to a plugin's profile that adds new things for buttons to do on the PC asks again before they run; plugin buttons in an imported profile ask too.
- The phone server listens only on this PC's own network addresses, keeps the phone's key out of addresses, and limits message sizes, slow and idle connections, and wrong pairing codes per address.
- The plugin DLL that runs is the one checked against its signature, and Install refuses a file that changed after it was looked at.
- Update signatures name the version they are for, so an older installer can't be passed off as a newer release.
- Pictures in imported profiles are checked like pictures you import by hand, with size limits when they are opened.
- Only the website and MinnionDeck's own editor page can talk to the website link.

## [0.2.1] - 2026-11-11

### Added
- **Choose pages on install.** Installing a plugin whose profile has choices (Streaming: OBS Studio, Twitch, YouTube) now asks which pages to add, from the Marketplace and from a plugin file with its `.mdprofile` beside it. A profile file imported by hand counts as that plugin's profile too, so its picks are remembered.

### Changed
- A plugin's settings for a service you didn't pick (for example YouTube's) are hidden. Plugins say which service a setting belongs to; settings that don't say are always shown.
- Removing a plugin and installing it again asks again, and replaces the profile you hadn't changed instead of adding a second copy. A profile you changed is kept as yours.

### Fixed
- A button's icon went blank after **Edit image**, **Sync**, then **Save**. Sync now also sends the editable design, which is kept (sealed, per picture, and included in backups) beside the picture, so Edit image opens the icon and background as separate layers again. Buttons synced before this have no stored design and still open as one image.

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

[0.2.2]: https://github.com/minnnion/MinnionDeck/compare/v0.2.1...v0.2.2
[0.2.0]: https://github.com/minnnion/MinnionDeck/compare/v0.1.2...v0.2.0
[0.1.2]: https://github.com/minnnion/MinnionDeck/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/minnnion/MinnionDeck/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/minnnion/MinnionDeck/releases/tag/v0.1.0

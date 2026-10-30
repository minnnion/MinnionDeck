<div align="center">

<img src="app-icon.png" alt="Minniondeck" width="96" height="96">

# Minniondeck

**Big square buttons and live screens for your PC, run from an Android phone.**
Press a button on the phone, and something happens on your PC, in your flight sim, or in the app in front.
Built for **Microsoft Flight Simulator 2024** first, with plugins for everything else.

![License](https://img.shields.io/badge/license-source--available-5fd0a8)
![Platform](https://img.shields.io/badge/platform-Windows%2010%2F11-7cc4ff)
![Built with](https://img.shields.io/badge/built%20with-Tauri%202%20%C2%B7%20Rust%20%C2%B7%20JS-b79cff)
![Version](https://img.shields.io/badge/version-1.0.1-ffc93c)

<img src="docs/screenshots/deck.png" alt="The Minniondeck desktop app showing a flight-sim profile" width="860">

</div>

---

## What it is

Minniondeck is a Stream-Deck-style system made of three parts:

| Part | What it does |
|---|---|
| **Desktop app** (this repo) | The hub. Holds your profiles, buttons, pictures and plugins, runs actions on the PC, and talks to your phones. |
| **Phone app** (Android, separate repo) | Shows the active profile as a deck of big square buttons and live screens. Presses go straight to the PC over Wi-Fi. |
| **Plugins** (separate repos) | Add whole worlds of buttons: MSFS 2024, Spotify, case lights, speech and more. |

The phone is the main device. Lay your deck out once on the PC, and the phone follows it, page for page.

## Features

### Deck and button designer
- **Pages, folders and profiles.** Grid layouts, buttons in three sizes, Go to page and Back, and an inspector for the selected button.
- **Pixel-art button designer.** Layers, button states (on and off and more), press animations, a GIF maker, a pixel editor, **1,036 pixel icons** and **3,466 app and game logos**, and live text.
- **Actions.** Keys, typing, mouse, programs (drop one on the Edit window), websites, volume and media, waits, sequences, knobs, live maps and plugin screens.
- **Automatic profile switching.** A profile can switch on by itself when an app such as OBS Studio or Spotify comes to the front.
- **Share profiles.** Export and import `.mdprofile` files, with an "Allow imported buttons?" check before anything runs.

<div align="center">
<img src="docs/screenshots/inspector.png" alt="A selected button and its inspector" width="780">
</div>

### Phones
- Pair with a matching number over Wi-Fi. The PC keeps only a hash of each phone's token, and phones pin the PC's certificate fingerprint.
- The phone follows the PC's page, stays awake, vibrates on press, and gets everything back when it reconnects.
- Go to page and Back turn the phone's page at once.

### Profiles
<div align="center">
<img src="docs/screenshots/profiles.png" alt="The Profiles page" width="780">
</div>

### Plugins
Plugins are signed `.mdplugin` files written with the Rust **plugin kit** in [`plugin-kit/`](plugin-kit).

- Each plugin gets its own settings form. Passwords are encrypted with Windows DPAPI and are never sent back to the window.
- Plugins can set a button's state, title, icon and live data (maps, screens). Phones get the same.
- Install, update and reload without restarting the app. Trusted authors and a developer mode are built in.
- A plugin can ask the app to speak. The app runs one local speech model (Phonon 31M English, Apache 2.0), so nothing leaves your PC.

| Plugin | What it does |
|---|---|
| **MSFS 2024 Deck** | Fenix A320 controls, live map, BeyondATC, GSX Pro, SimBrief |
| **Cockpit Camera** | Head motion and smart views for MSFS 2024, using the official Camera API |
| **Speech + Voices** | Offline text to speech with 37 English voices |
| **Case Lights** | MSI board RGB: colours, scenes, screen sync |
| **Spotify Control** | Play, skip, volume and a now-playing tile with the real cover |

> Plugins live in their own repositories and are not part of this one.

### Built for flight simulation
The MSFS 2024 plugin turns the phone into a cockpit panel:

- **Fenix A320:** every control as a button by panel, lit push buttons, switches, knobs with push and pull, and live values (FCU, BARO and more).
- **Live map:** your aircraft, other traffic, the SimBrief route, the ATC taxi route with hold-short bars, and airport diagrams (72,662 airports).
- **BeyondATC, GSX Pro and SimBrief:** radio messages, services, pushback direction, and the OFP screen. All read through the links those tools offer. Nothing in them is changed.
- **Cockpit camera:** head motion for engines, ground, air and touchdown, and smart views you reach by holding the middle mouse button.

## Installer and Plugin Store (in design)

An installer is being designed so a first run asks what you do, then downloads only the plugins, profiles and helpers that fit:

**Flight simmer, Truck simmer, Car racing simmer, or General / Stream Deck.**

The in-app **Plugin Store** will list official and community plugins live from GitHub. The images below are design previews, not final screens.

<div align="center">
<img src="docs/screenshots/installer-setup.png" alt="Installer design preview: pick your sim" width="420">
<img src="docs/screenshots/plugin-store.png" alt="Plugin Store design preview" width="420">
</div>

## Download

An official installer is coming and will be published under **Releases**. Minniondeck is only supported when installed from there, so it stays up to date and its plugins keep working.

**Requirements:** Windows 10 or 11 (64-bit) and an Android phone on the same Wi-Fi.

## Project layout

```
src/            Front end: plain JavaScript and CSS, in the app's own pixel style
src-tauri/      Rust backend: actions, phones, plugins, storage, speech, maps
plugin-kit/     Rust crate plugins are built against
public/         Static files the app loads (airport data, backend bridge)
docs/           Screenshots used in this README
```

## Security

Minniondeck is built to be safe to share profiles and plugins with:

- A **content security policy** stops imported data from running script, and every value from a file is checked before it reaches the page.
- Imported buttons ask first ("Allow imported buttons?"), and a phone press goes through the same question.
- Phone pairing uses 6-digit codes and 256-bit tokens from the OS random generator, with lockout after repeated tries and single-use codes.
- Plugin passwords are encrypted with Windows DPAPI.

To report a security problem, open a private security advisory on GitHub.

## Roadmap

- [x] Deck, profiles, phones and the button designer
- [x] Plugin system, with encrypted settings
- [x] Local speech engine
- [x] MSFS 2024 plugin: Fenix A320, live map, BeyondATC, GSX, SimBrief
- [x] Cockpit camera: head motion and smart views
- [ ] Installer with the "what do you sim?" setup
- [ ] Plugin Store with community plugins
- [ ] More languages

## Feedback

Bug reports and ideas are welcome as GitHub issues. The code is published so you can read it and see how it works. It is not accepting outside code changes.

## License

**Source-available, all rights reserved.** © 2026 minnnion. You may read the code and use the official release for your own use. You may not copy, rebuild, modify, redistribute or publish your own version of Minniondeck. See [LICENSE](LICENSE) for the exact terms.

Minniondeck is not affiliated with Microsoft, Asobo, Fenix Simulations, BeyondATC, FSDreamTeam, Spotify or any other product named here. All names and logos belong to their owners.

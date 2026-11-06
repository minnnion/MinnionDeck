# MinnionDeck

MinnionDeck: a free, pixel-art Stream Deck-style control panel for Windows, with phone control over your home Wi-Fi.

MinnionDeck turns your Windows PC into a programmable button panel. Arrange buttons on pages and profiles, give them pixel-art icons or your own pictures and GIFs, and press them with the mouse, the keyboard or a phone. Phones connect over your home Wi-Fi by scanning a QR code and pairing with a short code, so there's nothing to install on the phone. Plugins add actions such as opening programs, typing text, controlling volume and running key combinations.

## Download

Get `MinnionDeck_<version>_x64-setup.exe` from [Releases](https://github.com/minnnion/MinnionDeck/releases) and run it.

- You choose the folder. The app and everything it keeps (profiles, pictures, backups, plugins, settings) live together in that one folder.
- No admin rights needed. Uninstall from Windows' Installed apps; your data is kept unless you tick "Also delete my data".
- The app tells you when a new release is out.

## Features

- Pages and profiles of buttons, with pixel icons, pictures and GIFs, and buttons that change look by state
- Profiles that switch by themselves when you change apps
- Your phone as a deck: scan the QR code on the Phones page, press Allow on the PC, done
- Plugins from the Minnion Marketplace, installed from inside the app
- Import and export profiles (`.mdprofile`), backups you can restore
- Tray icon, close to tray, start with Windows

## Your data

Everything the app saves is encrypted (AES-256-GCM). Your own data (saves, pictures, backups) is sealed with a key made for your install the first time it runs, kept as `data.key` in the app's folder. Keep that file with your data: without it the data can't be opened. Shared profiles use a common key so they open on any PC.

## Install

MinnionDeck is installed only with the official setup from [Releases](https://github.com/minnnion/MinnionDeck/releases). The setup brings the app, its icon and Windows entries, the plugins you choose and their profiles. The source here is for reading; it is not meant to be built and run on its own.

## Plugins

Plugins live in their own repository and are installed from the Marketplace inside the app.

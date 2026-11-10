# Security

MinnionDeck runs on your PC, presses keys, opens programs and serves a page to phones on your
Wi-Fi, so security reports matter a lot to us.

## Reporting a problem

Please **don't open a public issue** for a security problem. Instead:

- use **[Report a vulnerability](https://github.com/minnnion/MinnionDeck/security/advisories/new)**
  on this repository (private, only the maintainer sees it), or
- email **minniondeck@gmail.com** with "Security" in the subject.

Say what you found, how to make it happen, and what an attacker could do with it. You'll get an
answer within 7 days, and a fix for anything serious goes out as a new release as soon as it's ready.

## Supported versions

Only the [latest release](https://github.com/minnnion/MinnionDeck/releases/latest) gets security fixes.
The app tells you when a new one is out (Settings › Updates).

## What MinnionDeck does to keep you safe

- **Signed updates.** An update installs only if its installer is signed with MinnionDeck's own key
  and matches GitHub's checksum.
- **Signed plugins.** A plugin runs without Developer mode only if a trusted author signed it and
  none of its files changed. Plugins run in a separate helper process.
- **Your data is encrypted** with a key made for your install and locked to your Windows account.
- **Phones pair** with a 6-digit code you confirm on the PC, and each phone gets its own key. The
  phone page serves your Wi-Fi only; turn on **Secure phone link** (Settings › Phones) to use HTTPS.
- **Imported buttons ask first** before they can press keys or open programs.
- Every change is built and tested on GitHub, and the dependencies are checked for known advisories
  every week.

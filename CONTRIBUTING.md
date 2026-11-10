# Helping MinnionDeck

Thanks for wanting to help! MinnionDeck is **source-available**: the code is here to read, and the
app is free to use, but it isn't open for copies or forks (see [LICENSE](LICENSE) and
[TERMS.md](TERMS.md)). Here is how you can help.

## Report a bug

- Open an issue with the **Bug report** form, or use the form on the
  [website](https://minniondeck.github.io/contribute/), which can attach your log file.
- Say which version you have (Settings › Updates), what you did, what you expected and what happened.
- The log is in `logs\` inside your MinnionDeck folder.

Found a security problem? Please read [SECURITY.md](SECURITY.md) instead of opening an issue.

## Ask for a feature

Open an issue with the **Feature idea** form, or start a thread in
[Discussions](https://github.com/minnnion/MinnionDeck/discussions). Tell us what you'd use it for:
that helps more than how it should work.

## Make a plugin

Plugins are welcome and are yours. They live in
[Minnion-Plugins](https://github.com/minnnion/Minnion-Plugins), where you'll find the plugin kit
and examples. When yours is ready, send it with **Submit a plugin** on the
[Marketplace page](https://minniondeck.github.io/marketplace/); approved plugins appear in the app's
Marketplace.

## Share profiles

Made a deck others would like (a game, a stream setup, an app)? Export it as a `.mdprofile`
(Profiles › Export) and share it in Discussions.

## Spread the word

A star on this repository, a review, or a short video of your deck helps more people find
MinnionDeck.

## Building from the source (to read and test)

You need Windows 10 or 11 and [Rust](https://rustup.rs/) (stable).

    cargo run --release      # run the app from the source
    cargo test               # the tests

The app keeps its data in `build\` beside the source when run this way.

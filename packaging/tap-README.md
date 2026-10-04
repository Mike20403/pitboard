# datlechin's Homebrew tap

[Pitboard](https://github.com/datlechin/pitboard) switches between your own Claude Code and
Codex logins and shows how much each one has left. This tap holds its two casks, `pitboard`
and `pitboard-app`.

Pitboard's release writes every file here from `packaging/` in Pitboard's repository. A
change made here is replaced at the next release.

## Install

```sh
brew install datlechin/tap/pitboard            # the command line, macOS and Linux
brew install --cask datlechin/tap/pitboard-app # the menu bar app, macOS 14 or later
```

The app includes the command line, so install one or the other. Homebrew refuses to install
one while the other is installed, because both put `pitboard` on your `PATH`.

## Upgrade from 0.3.0 or earlier

Before 0.4.0, the cask `pitboard` installed the app. From 0.4.0 it installs the command
line. To get the app back, with the command line inside it, run these in this order:

```sh
brew uninstall --cask pitboard
brew uninstall --formula --force pitboard
brew install --cask datlechin/tap/pitboard-app
```

Leave `--zap` out when you remove the old app cask. Its zap moves `~/.pitboard` to the
Trash, and that folder is the only record of your accounts and their parked logins. For the
old formula and for daily renewal, see
[Upgrade from 0.3.0 or earlier](https://docs.usepitboard.com/install/upgrade-from-0-3-0).

## Documentation

For installing without Homebrew and keeping Pitboard updated, see
[Install Pitboard](https://docs.usepitboard.com/install). Report a problem with either cask
as an [issue in Pitboard's repository](https://github.com/datlechin/pitboard/issues).

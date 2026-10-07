# Probe helpers

Three scripts the owner runs in the VM beside `pitboard-probe`, by hand and never in CI. They
touch only the scratch folder they are given, load no website, sign in to nothing, and print
counts, error codes and redacted paths, never what they read.

Copy this folder to `C:\probe\helpers`, then, in that folder:

```powershell
bun add proper-lockfile@4.1.2
```

- `reader-loop.mjs <scratch> [seconds] [hold-ms]`: the Bun or Node reader of the replace
  loop (VM C4). It waits for `pitboard-probe replace-loop`'s target, then opens it by path,
  reads it whole, checks the record and closes it, until the loop ends, and counts whole,
  torn, empty and missing reads.
- `proper-lockfile-loop.mjs <scratch> <hold|check> [seconds]`: proper-lockfile's lock with
  the options Claude Code's register records (VM C5). Run `hold` against
  `pitboard-probe lock --mode check`, then `check` against `pitboard-probe lock --mode hold`.
- `os-homedir.mjs`: `os.homedir()` and the home-shaped variables, with the profile shown as
  `<profile>` and the account name as `<user>` (VM B1, B2).

Run each under Bun 1.4.x and under Node LTS, and say which ran.

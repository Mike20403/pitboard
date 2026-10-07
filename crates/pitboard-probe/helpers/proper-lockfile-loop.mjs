// proper-lockfile's side of VM C5, with the options Claude Code's register records for its
// credential lock (stale 15000 ms, ten retries, 100 ms to 1000 ms of backoff, the heartbeat
// at half the staleness). Run it against the target `pitboard-probe lock` uses, so each side
// can see the other's lock:
//
//   hold:  take the lock and keep it for <seconds>, then say whether it was ever reported
//          compromised. Run `pitboard-probe lock --mode check` meanwhile.
//   check: for <seconds>, once a second, ask whether the lock is held and try to take it
//          without retrying. Run `pitboard-probe lock --mode hold` meanwhile.
//
// Usage: bun proper-lockfile-loop.mjs <scratch> <hold|check> [seconds]
// Install first, in this folder: bun add proper-lockfile@4.1.2
//
// It touches only `<scratch>\pitboard-probe-locktarget` and the lock beside it, and prints
// counts and error codes only.

import { existsSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import lockfile from "proper-lockfile";

const scratch = process.argv[2];
const mode = process.argv[3];
const seconds = Number(process.argv[4] ?? "30");
if (!scratch || (mode !== "hold" && mode !== "check")) {
  console.error("usage: proper-lockfile-loop.mjs <scratch> <hold|check> [seconds]");
  process.exit(2);
}
const target = join(scratch, "pitboard-probe-locktarget");
if (!existsSync(target)) writeFileSync(target, "x");

const options = {
  stale: 15000,
  update: 7500,
  retries: { retries: 10, minTimeout: 100, maxTimeout: 1000, factor: 2 },
  realpath: false,
};
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const runtime = typeof Bun !== "undefined" ? `bun ${Bun.version}` : `node ${process.version}`;

if (mode === "hold") {
  let compromised = null;
  try {
    const release = await lockfile.lock(target, {
      ...options,
      onCompromised: (err) => {
        compromised = String(err?.code ?? err?.message);
      },
    });
    await sleep(seconds * 1000);
    await release();
    console.log(JSON.stringify({ runtime, mode, acquired: true, compromised }));
  } catch (err) {
    console.log(JSON.stringify({ runtime, mode, acquired: false, code: String(err?.code) }));
    process.exit(1);
  }
} else {
  const counts = { held: 0, free: 0, locked_refusals: 0, other_errors: {} };
  const deadline = Date.now() + seconds * 1000;
  while (Date.now() < deadline) {
    const held = await lockfile.check(target, { stale: options.stale, realpath: false });
    counts[held ? "held" : "free"] += 1;
    try {
      const release = await lockfile.lock(target, { stale: options.stale, retries: 0, realpath: false });
      await release();
    } catch (err) {
      if (err?.code === "ELOCKED") counts.locked_refusals += 1;
      else counts.other_errors[String(err?.code)] = (counts.other_errors[String(err?.code)] ?? 0) + 1;
    }
    await sleep(1000);
  }
  console.log(JSON.stringify({ runtime, mode, ...counts }));
}

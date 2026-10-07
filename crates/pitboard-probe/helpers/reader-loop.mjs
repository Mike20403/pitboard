// The Bun (or Node) side of the replace loop (VM C4). It waits for the loop's target to
// appear, then opens it by path, reads it whole and closes it, again and again, the way a
// tool reads its login on each use, until the loop writes its `.done` file or the time runs
// out. Each read is checked against the loop's record format, so a torn read (a mix of two
// records, or a short one) is told from a whole one and from a missing file. Only counts and
// error codes are printed, never what was read.
//
// Usage: bun reader-loop.mjs <scratch> [seconds] [hold-ms]
//   <scratch>  the folder given to `pitboard-probe replace-loop --scratch`
//   hold-ms    keep each opened file open this long before closing it (default 0)
//
// libuv opens a file sharing read, write and delete (fs__open in src/win/fs.c); how Bun's
// node:fs opens one on Windows is part of what this measures, not assumed.

import { closeSync, existsSync, openSync, readSync } from "node:fs";
import { join } from "node:path";

const scratch = process.argv[2];
const seconds = Number(process.argv[3] ?? "120");
const holdMs = Number(process.argv[4] ?? "0");
if (!scratch) {
  console.error("usage: reader-loop.mjs <scratch> [seconds] [hold-ms]");
  process.exit(2);
}
const target = join(scratch, "pitboard-probe-replace-target");
const done = join(scratch, "pitboard-probe-replace-target.done");
const RECORD_LEN = 4096;

// The loop's record: "PBRL", the round (u32 LE), a filler of round % 251, and an FNV-1a
// checksum (u32 LE) of everything before it.
function check(buf) {
  if (buf.length === 0) return "empty";
  if (buf.length !== RECORD_LEN || buf.toString("latin1", 0, 4) !== "PBRL") return "torn";
  let h = 0x811c9dc5;
  for (let i = 0; i < RECORD_LEN - 4; i++) {
    h ^= buf[i];
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  if (h !== buf.readUInt32LE(RECORD_LEN - 4)) return "torn";
  const fill = buf.readUInt32LE(4) % 251;
  for (let i = 8; i < RECORD_LEN - 4; i++) if (buf[i] !== fill) return "torn";
  return "whole";
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const waitStart = Date.now();
while (!existsSync(target) && Date.now() - waitStart < 60_000) await sleep(100);
if (!existsSync(target)) {
  console.log(JSON.stringify({ started: false, reason: "the target never appeared" }));
  process.exit(1);
}

const counts = { reads: 0, whole: 0, torn: 0, empty: 0 };
const openErrors = {};
const readErrors = {};
const deadline = Date.now() + seconds * 1000;
const buf = Buffer.alloc(RECORD_LEN * 2);
while (Date.now() < deadline && !existsSync(done)) {
  counts.reads += 1;
  let fd;
  try {
    fd = openSync(target, "r");
  } catch (e) {
    const code = String(e?.code);
    openErrors[code] = (openErrors[code] ?? 0) + 1;
    continue;
  }
  try {
    let total = 0;
    for (;;) {
      const n = readSync(fd, buf, total, buf.length - total, total);
      if (n === 0 || total + n >= buf.length) {
        total += n;
        break;
      }
      total += n;
    }
    counts[check(buf.subarray(0, total))] += 1;
    if (holdMs > 0) await sleep(holdMs);
  } catch (e) {
    const code = String(e?.code);
    readErrors[code] = (readErrors[code] ?? 0) + 1;
  } finally {
    try {
      closeSync(fd);
    } catch {
      // already closed
    }
  }
}

const runtime = typeof Bun !== "undefined" ? `bun ${Bun.version}` : `node ${process.version}`;
console.log(
  JSON.stringify({ runtime, hold_ms: holdMs, ...counts, missing_by_open_error: openErrors, read_errors_by_code: readErrors }),
);

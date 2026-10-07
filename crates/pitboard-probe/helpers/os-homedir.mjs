// VM B1 and B2 from Bun's (or Node's) side: os.homedir(), the home-shaped variables, and
// whether each follows an overridden USERPROFILE. The account is hidden as the probe hides
// it: the profile folder Windows reports for the account (os.userInfo().homedir) prints as
// <profile> and the account name as <user>.
//
// Usage: bun os-homedir.mjs   (and: node os-homedir.mjs)

import os from "node:os";

const runtime = typeof Bun !== "undefined" ? `bun ${Bun.version}` : `node ${process.version}`;
const info = os.userInfo();
const profile = info.homedir;
const user = info.username;

function redact(value) {
  if (value == null) return null;
  let v = String(value);
  const lower = v.toLowerCase();
  const p = profile.toLowerCase();
  let out = "";
  let i = 0;
  while (i < v.length) {
    const end = i + p.length;
    const boundary = end === v.length || v[end] === "\\" || v[end] === "/";
    if (p.length > 0 && lower.startsWith(p, i) && boundary) {
      out += "<profile>";
      i = end;
    } else {
      out += v[i];
      i += 1;
    }
  }
  return out
    .split(/([\\/])/)
    .map((part) => (part.toLowerCase() === user.toLowerCase() ? "<user>" : part))
    .join("");
}

const home = os.homedir();
console.log(
  JSON.stringify(
    {
      runtime,
      os_homedir: redact(home),
      os_homedir_is_the_account_profile: home.toLowerCase() === profile.toLowerCase(),
      os_homedir_follows_USERPROFILE: home === process.env.USERPROFILE,
      os_homedir_is_nfc: home === home.normalize("NFC"),
      env: Object.fromEntries(
        [
          "USERPROFILE",
          "HOME",
          "HOMEDRIVE",
          "HOMEPATH",
          "APPDATA",
          "LOCALAPPDATA",
          "CODEX_HOME",
          "CLAUDE_CONFIG_DIR",
          "PITBOARD_HOME",
        ].map((k) => [k, redact(process.env[k])]),
      ),
    },
    null,
    2,
  ),
);

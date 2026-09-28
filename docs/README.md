# pitboard documentation

The source of docs.usepitboard.com, built with Mintlify. Pages are `.mdx` files; the
sidebar and site settings are in `docs.json`. Writing rules are in `AGENTS.md`.

## Preview

Install the Mintlify CLI once, then run it in this folder:

    npm i -g mint
    mint dev

The site opens at http://localhost:3000. The first run downloads Mintlify's preview
client. Restart `mint dev` after changing `docs.json`.

## Check

All three must pass before a commit:

    mint validate
    mint broken-links --check-anchors --check-redirects
    mint a11y

## Where things go

- Pages: the folder that matches their sidebar group, such as `guides/switch.mdx`. Add
  each page to `docs.json`.
- Images: `images/`, referenced as `/images/name.png`.
- The site icon: `favicon.svg`, drawn from `apple/scripts/make-icon.swift`.
- Files Mintlify must not publish: `.mintignore`.

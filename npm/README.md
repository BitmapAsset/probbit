# pbit (npm)

Installs the prebuilt `pbit` command-line binary: a virtual p-bit processor that returns joint decisions under hard
rules, with gated odds. JSON in, JSON out. Source, documentation and every benchmark:
<https://github.com/BitmapAsset/pbit>.

```sh
npm install -g pbit
pbit demo --tasks 12 | pbit decide --pretty
pbit stats --pretty
```

The install step fetches the release archive for your platform from GitHub Releases (macOS arm64 and x86_64, Linux
x86_64 with glibc, Windows x86_64), checks it against the `.sha256` file published beside it and keeps the binary inside
this package. No dependencies; Node >= 18. If the install step could not run (`--ignore-scripts`, offline), the first
`pbit` call fetches the binary. A checksum mismatch fails the install and installs nothing.

| variable | effect |
|---|---|
| `PBIT_VERSION` | release tag to fetch (default: `v` + this package's version) |
| `PBIT_DOWNLOAD_BASE` | fetch `$PBIT_DOWNLOAD_BASE/<tag>/pbit-<tag>-<target>.tar.gz` (`.zip` on Windows) instead of GitHub |
| `PBIT_TARGET` | Rust target triple to fetch instead of the detected one |
| `PBIT_BINARY` | at install: copy this binary instead of downloading; at run time: run this binary |

`pbit`'s exit code passes through unchanged: 0 answer (`exact`, `diagnostics_passed`, `partial`), 1 `infeasible`
(a proof), 2 bad input or flag, 3 `refused` / `declined`. Calling it from Node code (spawn, JSON on stdin, exit codes
mapped): `examples/node/decide.mjs` in the repository; from other languages and agent harnesses: `docs/agents.md`.

License: Apache-2.0.

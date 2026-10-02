# probbit (npm)

Installs the prebuilt `probbit` command-line binary: a virtual p-bit processor that returns joint decisions under hard
rules, with gated odds. JSON in, JSON out. Source, documentation and every benchmark:
<https://github.com/BitmapAsset/probbit>.

```sh
npm install -g probbit
probbit demo --tasks 12 | probbit decide --pretty
probbit stats --pretty
```

The install step fetches the release archive for your platform from GitHub Releases (macOS arm64 and x86_64, Linux
x86_64 with glibc, Windows x86_64), checks it against the `.sha256` file published beside it and keeps the binary inside
this package. No dependencies; Node >= 18. If the install step could not run (`--ignore-scripts`, offline), the first
`probbit` call fetches the binary. A checksum mismatch fails the install and installs nothing.

| variable | effect |
|---|---|
| `PROBBIT_VERSION` | release tag to fetch (default: `v` + this package's version) |
| `PROBBIT_DOWNLOAD_BASE` | fetch `$PROBBIT_DOWNLOAD_BASE/<tag>/probbit-<tag>-<target>.tar.gz` (`.zip` on Windows) instead of GitHub |
| `PROBBIT_TARGET` | Rust target triple to fetch instead of the detected one |
| `PROBBIT_BINARY` | at install: copy this binary instead of downloading; at run time: run this binary |

`probbit`'s exit code passes through unchanged: 0 answer (`exact`, `diagnostics_passed`, `partial`), 1 `infeasible`
(a proof), 2 bad input or flag, 3 `refused` / `declined`. Calling it from Node code (spawn, JSON on stdin, exit codes
mapped): `examples/node/decide.mjs` in the repository; from other languages and agent harnesses: `docs/agents.md`.

License: Apache-2.0.

# Security policy

## Supported versions

| version | supported |
|---|---|
| 0.1.x | yes |

## Reporting a vulnerability

Please do not open a public issue for a security problem. Use GitHub's private
vulnerability reporting instead:

**https://github.com/BitmapAsset/pbit/security/advisories/new**

Include the version (`pbit version`), the operating system, the exact command,
and an input that reproduces the problem (the problem JSON, or the `pbit demo`
flags and seed). We aim to acknowledge a report within a week and to keep you
informed while it is being fixed. Credit goes to the reporter in the release
notes unless you prefer otherwise.

## What counts

pbit is a local command-line processor that reads JSON from standard input and
writes JSON to standard output. It opens no network connections and has no
external crates. Reports in these areas are especially welcome:

- **Input handling.** A malformed or hostile problem document must exit 2 with a
  message on stderr. A crash (`abort`, stack overflow, panic), a hang, unbounded
  memory growth that `--mem-limit-mb` does not bound, or any other way an input
  can take the process down is a bug we treat as security-relevant, because pbit
  is meant to be fed data from other programs.
- **Resource controls.** `--cpu-limit`, `--mem-limit-mb`, `--threads` and
  `--priority` exist so pbit can share a machine. A way to escape those limits is
  in scope.
- **Wrong answers labelled `exact`.** The exact tiers claim exactness; a program
  on which they return a wrong answer without declining is a correctness bug and
  we want to know about it, whether or not it has a security angle.

Out of scope: the statistical nature of `certified` (it is a calibrated bound,
not a proof; see README), and performance that merely falls short of the
numbers in `BENCHMARKS.md` on different hardware.

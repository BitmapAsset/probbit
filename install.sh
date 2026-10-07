#!/bin/sh
# probbit installer: fetches a prebuilt release archive, checks its SHA-256 and installs the `probbit` binary. Never uses sudo.
#
#   curl -fsSL https://raw.githubusercontent.com/BitmapAsset/probbit/main/install.sh | sh
#   curl -fsSL https://raw.githubusercontent.com/BitmapAsset/probbit/main/install.sh | PROBBIT_VERSION=v0.8.0 sh
#
# Environment (all optional):
#   PROBBIT_VERSION        release tag, e.g. v0.8.0 (default: the latest release)
#   PROBBIT_INSTALL_DIR    where `probbit` goes (default: /usr/local/bin if you can write there, else ~/.local/bin)
#   PROBBIT_DOWNLOAD_BASE  the archive is fetched from $PROBBIT_DOWNLOAD_BASE/<tag>/probbit-<tag>-<target>.tar.gz
#                       (default: https://github.com/BitmapAsset/probbit/releases/download); needs PROBBIT_VERSION
#   PROBBIT_TARGET         Rust target triple to fetch instead of the detected one
# Needs curl or wget, tar, and one of sha256sum, shasum or openssl.
set -eu

REPO="BitmapAsset/probbit"

say() { printf '%s\n' "$*"; }
die() { printf 'probbit install: error: %s\n' "$*" >&2; exit 1; }
has() { command -v "$1" > /dev/null 2>&1; }

fetch() { # fetch URL FILE
    if has curl; then
        case "$1" in
            https://*) curl --proto '=https' --proto-redir '=https' -fsSL "$1" -o "$2" ;;
            *) curl -fsSL "$1" -o "$2" ;;
        esac
    elif has wget; then
        wget -q -O "$2" "$1"
    else
        die "need curl or wget"
    fi
}

sha256() {
    if has sha256sum; then sha256sum "$1" | cut -d ' ' -f 1
    elif has shasum; then shasum -a 256 "$1" | cut -d ' ' -f 1
    elif has openssl; then openssl dgst -sha256 "$1" | sed 's/^.*= *//'
    else die "need sha256sum, shasum or openssl to check the download"
    fi
}

detect_target() {
    os=$(uname -s)
    arch=$(uname -m)
    case "$os" in
        Darwin)
            # a shell running under Rosetta reports x86_64 on Apple silicon: take the native build
            if [ "$arch" = x86_64 ] && [ "$(sysctl -n sysctl.proc_translated 2> /dev/null || echo 0)" = 1 ]; then arch=arm64; fi
            case "$arch" in
                arm64 | aarch64) echo aarch64-apple-darwin ;;
                x86_64) echo x86_64-apple-darwin ;;
                *) die "no prebuilt probbit for macOS on $arch" ;;
            esac
            ;;
        Linux)
            if (ldd --version 2>&1 || true) | grep -qi musl; then
                die "this system uses musl libc (Alpine?) and the release ships a glibc build only; build from source: cargo install --git https://github.com/$REPO probbit-cli"
            fi
            case "$arch" in
                x86_64 | amd64) echo x86_64-unknown-linux-gnu ;;
                *) die "no prebuilt probbit for Linux on $arch yet; build from source: cargo install --git https://github.com/$REPO probbit-cli" ;;
            esac
            ;;
        MINGW* | MSYS* | CYGWIN* | Windows_NT) die "on Windows use install.ps1 (PowerShell)" ;;
        *) die "no prebuilt probbit for $os; build from source: cargo install --git https://github.com/$REPO probbit-cli" ;;
    esac
}

latest_tag() {
    api="https://api.github.com/repos/$REPO/releases/latest"
    fetch "$api" "$tmp/latest.json" || die "could not look up the latest release at $api (set PROBBIT_VERSION)"
    sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' "$tmp/latest.json" | head -n 1
}

main() {
    base="${PROBBIT_DOWNLOAD_BASE:-https://github.com/$REPO/releases/download}"
    base="${base%/}"
    target="${PROBBIT_TARGET:-}"
    if [ -z "$target" ]; then target=$(detect_target) || exit 1; fi

    tmp=$(mktemp -d 2> /dev/null || mktemp -d -t probbit)
    trap 'rm -rf "$tmp"' EXIT
    trap 'exit 130' INT
    trap 'exit 143' TERM

    version="${PROBBIT_VERSION:-}"
    if [ -z "$version" ]; then
        [ -z "${PROBBIT_DOWNLOAD_BASE:-}" ] || die "set PROBBIT_VERSION (e.g. v0.8.0) together with PROBBIT_DOWNLOAD_BASE"
        version=$(latest_tag) || exit 1
        [ -n "$version" ] || die "no published release found (set PROBBIT_VERSION)"
    fi
    case "$version" in v*) ;; *) version="v$version" ;; esac

    name="probbit-$version-$target"
    asset="$name.tar.gz"
    url="$base/$version/$asset"
    say "probbit install: $version for $target"
    say "  fetching $url"
    fetch "$url" "$tmp/$asset" || die "download failed: $url"
    fetch "$url.sha256" "$tmp/$asset.sha256" || die "download failed: $url.sha256"

    want=$(cut -d ' ' -f 1 < "$tmp/$asset.sha256" | tr 'ABCDEF' 'abcdef')
    got=$(sha256 "$tmp/$asset" | tr 'ABCDEF' 'abcdef')
    case "$want" in *[!0-9a-f]* | '') die "unreadable checksum file $asset.sha256" ;; esac
    [ "${#want}" -eq 64 ] || die "unreadable checksum file $asset.sha256"
    [ "$want" = "$got" ] || die "checksum mismatch for $asset: expected $want, got $got (nothing was installed)"
    say "  sha256 ok  $got"

    tar -xzf "$tmp/$asset" -C "$tmp" || die "could not unpack $asset"
    [ -f "$tmp/$name/probbit" ] || die "$asset has no $name/probbit"

    if [ -n "${PROBBIT_INSTALL_DIR:-}" ]; then
        dir="$PROBBIT_INSTALL_DIR"
    elif [ -d /usr/local/bin ] && [ -w /usr/local/bin ]; then
        dir=/usr/local/bin
    else
        dir="${HOME:?HOME is not set; set PROBBIT_INSTALL_DIR}/.local/bin"
    fi
    mkdir -p "$dir" 2> /dev/null || die "cannot create $dir (set PROBBIT_INSTALL_DIR to a directory you can write)"
    [ -w "$dir" ] || die "cannot write to $dir (set PROBBIT_INSTALL_DIR to a directory you can write; this script never uses sudo)"
    cp "$tmp/$name/probbit" "$dir/.probbit.$$" && chmod 755 "$dir/.probbit.$$" && mv -f "$dir/.probbit.$$" "$dir/probbit" \
        || die "could not install into $dir"
    ran=$("$dir/probbit" version 2>&1) || die "installed $dir/probbit, but it does not run here: $ran"
    say "  installed  $dir/probbit ($ran)"

    cmd=probbit
    case ":$PATH:" in
        *":$dir:"*) ;;
        *)
            cmd="$dir/probbit"
            say ""
            say "$dir is not on your PATH. Add it (e.g. in ~/.profile):"
            say "  export PATH=\"$dir:\$PATH\""
            ;;
    esac
    say ""
    # At a terminal, the banner is probbit's own hero screen (its theme: NO_COLOR or PROBBIT_THEME=plain turn the colour off);
    # releases before 0.3.0 have none (a bare `probbit` exits 2 there), and a pipe gets the plain next steps
    if [ -t 1 ] && "$dir/probbit" 2> /dev/null; then return 0; fi
    say "Next:"
    say "  $cmd version"
    say "  $cmd demo --tasks 12 | $cmd decide --pretty"
    say "  $cmd stats --pretty"
}

main "$@"

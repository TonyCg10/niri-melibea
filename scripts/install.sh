#!/bin/sh
set -eu

# install.sh — build this niri in release, install it, then drop the build tree.
#
# The binary goes to $CELESTINA_NIRI_PREFIX/niri (default
# ~/.local/lib/celestina), the compositor the author's session runs. It is
# copied to a temporary file and renamed over the old one, so the running
# compositor keeps its open binary and loads the new one at the next login;
# this script never restarts the session.
#
# Once the installed bytes are checked against the build, target/ holds only a
# cache that the next build regenerates (tens of gigabytes of debug and
# release objects), so it is removed. Pass --keep-target to leave it, for
# example while testing nested builds.

usage() {
    echo "usage: scripts/install.sh [--keep-target]" >&2
    exit 2
}

keep_target=0
case $# in
    0) ;;
    1) [ "$1" = --keep-target ] || usage; keep_target=1 ;;
    *) usage ;;
esac

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
install_dir=${CELESTINA_NIRI_PREFIX:-$HOME/.local/lib/celestina}
destination=$install_dir/niri

(cd "$repo_root" && cargo build --release --locked)
built=$repo_root/target/release/niri

mkdir -p -- "$install_dir"
temporary=$(mktemp "$install_dir/.niri.XXXXXX")
trap 'rm -f -- "$temporary"' EXIT HUP INT TERM
install -m 0755 "$built" "$temporary"
mv -f -- "$temporary" "$destination"
trap - EXIT HUP INT TERM
cmp -s -- "$built" "$destination" || {
    echo "install: $destination does not match $built" >&2
    exit 1
}
echo ">> installed $destination; it takes effect at the next login" >&2

if [ "$keep_target" -eq 0 ]; then
    (cd "$repo_root" && cargo clean --quiet)
    echo ">> removed target/; the next build regenerates it" >&2
fi

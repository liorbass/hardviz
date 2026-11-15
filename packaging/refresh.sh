#!/usr/bin/env bash
# Re-points every packaging manifest at a published release and refreshes its
# hashes, so cutting a release does not mean hand-editing six files.
#
#   packaging/refresh.sh v0.1.6
#
# The release assets must exist already (this downloads and hashes them).
# Written for GNU sed and GNU date; on macOS install gnu-sed or adapt -i.
set -euo pipefail

repo="liorbass/hardviz"
ver="${1:?usage: packaging/refresh.sh vX.Y.Z}"
tag="${ver#v}"
base="https://github.com/${repo}/releases/download/${ver}"
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$here"

for t in curl sha256sum sed awk date; do
  command -v "$t" >/dev/null || { echo "missing tool: $t" >&2; exit 1; }
done

declare -A sha
for a in hardviz-windows-x64.zip hardviz-linux-x64.tar.gz hardviz-macos-arm64.tar.gz; do
  printf 'hashing %-28s ' "$a"
  sha[$a]="$(curl -sSL "${base}/${a}" | sha256sum | cut -d' ' -f1)"
  printf '%s\n' "${sha[$a]}"
done

# The .desktop file ships from this repo, so its hash comes from disk.
desktop_sha="$(sha256sum aur/hardviz.desktop | cut -d' ' -f1)"

# winget wants an upper-case digest; the others take the usual lower case.
upper() { printf '%s' "$1" | tr '[:lower:]' '[:upper:]'; }

# The release date is when GitHub published the tag, falling back to today.
release_date="$(gh api "repos/${repo}/releases/tags/${ver}" --jq .published_at 2>/dev/null || true)"
release_date="${release_date%%T*}"
test -n "$release_date" || release_date="$(date -u +%F)"

set_field() { # set_field <file> <key> <value> [indent] — rewrites the first matching "key: value" line
  # With [indent], only a line whose leading whitespace is exactly that counts.
  # A Scoop manifest has two other "url" keys — under "license" and under
  # "autoupdate" — both nested one level deeper than the one being rewritten.
  local f="$1" k="$2" v="$3" tmp exact=0 want=""
  if [ $# -ge 4 ]; then exact=1; want="$4"; fi
  test -f "$f" || { echo "no such manifest: $f" >&2; exit 1; }
  tmp="$(mktemp)"
  awk -v k="$k" -v v="$v" -v exact="$exact" -v want="$want" '
    {
      if (!done) {
        match($0, /^[ \t]*/)
        ind  = substr($0, 1, RLENGTH)
        head = substr($0, RLENGTH + 1)
        if ((exact == 0 || ind == want) && index(head, k) == 1) {
          printf "%s%s%s\n", ind, k, v
          done = 1
          next
        }
      }
      print
    }
    END { if (!done) { printf "%s: no \"%s\" line\n", FILENAME, k > "/dev/stderr"; exit 1 } }
  ' "$f" > "$tmp" && mv "$tmp" "$f"
}

set_aur_sums() { # rewrites the whole sha256sums=( ... ) block
  local f=aur/PKGBUILD tmp
  tmp="$(mktemp)"
  awk -v h1="$1" -v h2="$2" '
    /^sha256sums=\(/ { print; printf "  \x27%s\x27\n  \x27%s\x27\n", h1, h2; skip=1; next }
    skip && /^\)/     { skip=0; print; next }
    skip              { next }
                      { print }
  ' "$f" > "$tmp"
  grep -q "sha256sums=(" "$f" || { echo "${f}: no sha256sums block" >&2; rm -f "$tmp"; exit 1; }
  mv "$tmp" "$f"
}

wingetdir="winget/LiorBass.HardViz/${tag}"
test -d "winget/LiorBass.HardViz" || { echo "no winget manifests" >&2; exit 1; }
for f in winget/LiorBass.HardViz/*/*.yaml; do
  sed -i "s/^PackageVersion: .*/PackageVersion: ${tag}/" "$f"
done

installer="${wingetdir}/LiorBass.HardViz.installer.yaml"
set_field "$installer" "InstallerUrl: " "${base}/hardviz-windows-x64.zip"
set_field "$installer" "InstallerSha256: " "$(upper "${sha[hardviz-windows-x64.zip]}")"
# Quoted on purpose: a bare 2026-11-11 is a YAML date, and the schema wants a
# string — wingetcreate's validator rejects the unquoted form.
set_field "$installer" "ReleaseDate: " "\"${release_date}\""

set_field scoop/hardviz.json '"version": ' "\"${tag}\"," "  "
set_field scoop/hardviz.json '"url": '     "\"${base}/hardviz-windows-x64.zip\"," "  "
set_field scoop/hardviz.json '"hash": '    "\"${sha[hardviz-windows-x64.zip]}\"," "  "

sed -i "s/^pkgver=.*/pkgver=${tag}/" aur/PKGBUILD
set_aur_sums "${sha[hardviz-linux-x64.tar.gz]}" "$desktop_sha"

# The cask's url interpolates #{version}, so only the version and hash change.
set_field homebrew-tap/Casks/hardviz.rb 'version ' "\"${tag}\""
set_field homebrew-tap/Casks/hardviz.rb 'sha256 '  "\"${sha[hardviz-macos-arm64.tar.gz]}\""

echo
echo "Pointed the manifests at ${ver} (released ${release_date})."
echo "If the winget folder is still named for the old version, rename it:"
echo "  mv winget/LiorBass.HardViz/<old> ${wingetdir}"
echo "Then review 'git diff packaging/' and follow packaging/README.md to submit."

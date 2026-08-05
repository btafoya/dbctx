#!/usr/bin/env bash
#
# Cuts a dbctx release: bumps Cargo.toml, cuts CHANGELOG.md, runs the CI
# gates locally, tags, publishes to crates.io, and pushes.
#
# Usage: scripts/release.sh <version>   e.g. scripts/release.sh 1.0.0

set -euo pipefail

VERSION="${1:?Usage: scripts/release.sh <version>   e.g. scripts/release.sh 1.0.0}"
TAG="v${VERSION}"
DATE="$(date +%Y-%m-%d)"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

confirm() {
    read -r -p "$1 [y/N] " reply
    [[ "$reply" =~ ^[Yy]$ ]]
}

echo "==> Releasing dbctx ${VERSION} (tag ${TAG})"

# --- crates.io credentials ------------------------------------------------
CARGO_CREDENTIALS="${CARGO_HOME:-$HOME/.cargo}/credentials.toml"
if [[ ! -f "$CARGO_CREDENTIALS" ]]; then
    echo "No crates.io credentials found at ${CARGO_CREDENTIALS}." >&2
    echo "Get a token from https://crates.io/settings/tokens, then run:" >&2
    echo "  cargo login <token>" >&2
    exit 1
fi

# --- repo state ------------------------------------------------------------
if [[ -n "$(git status --porcelain)" ]]; then
    echo "Working tree is not clean:" >&2
    git status --short
    exit 1
fi

BRANCH="$(git rev-parse --abbrev-ref HEAD)"
if [[ "$BRANCH" != "main" ]]; then
    echo "Not on main (currently on '${BRANCH}')."
    confirm "Continue anyway?" || exit 1
fi

if git rev-parse "$TAG" >/dev/null 2>&1; then
    echo "Tag ${TAG} already exists." >&2
    exit 1
fi

# --- bump Cargo.toml version ------------------------------------------------
CURRENT_VERSION="$(grep -m1 '^version = ' Cargo.toml | sed -E 's/version = "(.*)"/\1/')"
if [[ "$CURRENT_VERSION" != "$VERSION" ]]; then
    echo "==> Bumping Cargo.toml version ${CURRENT_VERSION} -> ${VERSION}"
    sed -i "0,/^version = \"${CURRENT_VERSION}\"/s//version = \"${VERSION}\"/" Cargo.toml
else
    echo "==> Cargo.toml is already at ${VERSION}"
fi

# --- cut CHANGELOG.md -------------------------------------------------------
if grep -q "^## \[${VERSION}\]" CHANGELOG.md; then
    echo "==> CHANGELOG.md already has a [${VERSION}] section, leaving it as-is"
else
    echo "==> Cutting CHANGELOG.md section for ${VERSION}"
    awk -v ver="$VERSION" -v date="$DATE" '
        !done && /^## \[Unreleased\]/ {
            print
            print ""
            print "## [" ver "] - " date
            done = 1
            next
        }
        { print }
    ' CHANGELOG.md > CHANGELOG.md.tmp
    mv CHANGELOG.md.tmp CHANGELOG.md
fi

# --- quality gates (same as CI, see CLAUDE.md) ------------------------------
echo "==> cargo fmt --check"
cargo fmt --all --check

echo "==> cargo clippy"
cargo clippy --all-targets --all-features -- -D warnings

echo "==> cargo nextest run"
cargo nextest run --all-features

echo "==> cargo test --doc"
cargo test --doc --all-features

# --- commit, tag -------------------------------------------------------------
git add Cargo.toml Cargo.lock CHANGELOG.md
git commit -m "Release ${VERSION}"
git tag -a "$TAG" -m "dbctx ${VERSION}"

# --- publish dry run, then the real thing -----------------------------------
echo "==> cargo publish --dry-run"
cargo publish --dry-run

echo
echo "Dry run passed. This next step is irreversible: crates.io versions"
echo "can be yanked but never deleted or overwritten."
if confirm "Publish ${VERSION} to crates.io and push ${TAG} to origin?"; then
    cargo publish
    git push origin "$BRANCH"
    git push origin "$TAG"
    echo "==> Released dbctx ${VERSION}."
    echo "Remember to: update README.md's install section back to"
    echo "'cargo install dbctx' and add a crates.io version badge."
else
    echo "Stopped before publishing. Local commit and tag ${TAG} are in place;"
    echo "re-run 'cargo publish' and 'git push origin ${BRANCH} --tags' manually when ready,"
    echo "or 'git tag -d ${TAG}' and 'git reset --hard HEAD~1' to undo."
fi

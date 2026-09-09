#!/usr/bin/env bash
set -uo pipefail

# Release process:
#   1. ./scripts/release.sh [patch|minor|major]
#   2. Trigger "Publish to npm" workflow on GitHub Actions
#
# NOTE: sed -i '' is BSD/macOS syntax. This script is meant to be run locally.

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PACKAGE_JSON="$REPO_ROOT/bindings/typescript/parser/package.json"
PACKAGE_LOCK="$REPO_ROOT/bindings/typescript/parser/package-lock.json"
CARGO_TOML="$REPO_ROOT/Cargo.toml"
CARGO_LOCK="$REPO_ROOT/Cargo.lock"

# The npm package and the Rust crate ship under one tag, so they carry the same
# version. The two lockfiles are regenerated from them, never edited.
VERSION_FILES=("$PACKAGE_JSON" "$PACKAGE_LOCK" "$CARGO_TOML" "$CARGO_LOCK")

cargo_toml_version() {
  awk -F'"' '/^version = "/ { print $2; exit }' "$CARGO_TOML"
}

cargo_lock_version() {
  awk '/^name = "motly-rust"$/ { getline; gsub(/["]/, "", $3); print $3; exit }' "$CARGO_LOCK"
}

BUMP="${1:-patch}"

echo ""
echo "=== MOTLY Release ==="
echo ""

# --- Preflight checks ---

# Clean working tree?
if ! git -C "$REPO_ROOT" diff --quiet || ! git -C "$REPO_ROOT" diff --cached --quiet; then
  echo "STOP: Working tree is not clean."
  echo "Commit or stash your changes first."
  exit 1
fi

# On main?
BRANCH=$(git -C "$REPO_ROOT" branch --show-current)
if [ "$BRANCH" != "main" ]; then
  echo "STOP: Not on main (currently on '$BRANCH')."
  exit 1
fi

# Up to date with remote?
git -C "$REPO_ROOT" fetch origin main --quiet
LOCAL=$(git -C "$REPO_ROOT" rev-parse HEAD)
REMOTE=$(git -C "$REPO_ROOT" rev-parse origin/main)
if [ "$LOCAL" != "$REMOTE" ]; then
  echo "STOP: Local main and origin/main have diverged."
  echo "  local:  $LOCAL"
  echo "  remote: $REMOTE"
  echo "Pull or push first."
  exit 1
fi

# --- Compute version ---

CURRENT=$(jq -r .version "$PACKAGE_JSON")
CARGO_CURRENT=$(cargo_toml_version)
if [ "$CURRENT" != "$CARGO_CURRENT" ]; then
  echo "STOP: package.json is $CURRENT but Cargo.toml is $CARGO_CURRENT."
  echo "They ship under one tag and must agree. Reconcile them first."
  exit 1
fi

IFS='.' read -r MAJOR MINOR PATCH <<< "$CURRENT"

case "$BUMP" in
  patch) PATCH=$((PATCH + 1)) ;;
  minor) MINOR=$((MINOR + 1)); PATCH=0 ;;
  major) MAJOR=$((MAJOR + 1)); MINOR=0; PATCH=0 ;;
  *) echo "Usage: $0 [patch|minor|major] (default: patch)"; exit 1 ;;
esac

NEW="$MAJOR.$MINOR.$PATCH"
TAG="v$NEW"

# Tag collision?
if git -C "$REPO_ROOT" tag -l "$TAG" | grep -q .; then
  echo "STOP: Tag $TAG already exists."
  exit 1
fi

echo "  Preflight OK (clean tree, on main, up to date)"
echo "  Version: $CURRENT -> $NEW"
echo ""

# --- Update version in source files ---

revert_version_files() {
  git -C "$REPO_ROOT" checkout -- "${VERSION_FILES[@]}" 2>/dev/null
}

abort() {
  echo ""
  echo "FAILED: $1"
  revert_version_files
  echo "  Version files reverted. Nothing was committed."
  exit 1
}

require_version() {
  [ "$2" = "$NEW" ] || abort "$1 reads $2, expected $NEW."
}

sed -i '' "s/\"version\": \"$CURRENT\"/\"version\": \"$NEW\"/" "$PACKAGE_JSON"
sed -i '' "s/^version = \"$CURRENT\"/version = \"$NEW\"/" "$CARGO_TOML"

# sed exits 0 when it matches nothing, so confirm each file actually moved.
require_version "package.json" "$(jq -r .version "$PACKAGE_JSON")"
require_version "Cargo.toml" "$(cargo_toml_version)"

echo "  Regenerating lockfiles..."
(cd "$REPO_ROOT/bindings/typescript/parser" && npm install --package-lock-only --loglevel error) \
  || abort "Could not regenerate package-lock.json."
(cd "$REPO_ROOT" && cargo check --quiet) || abort "Could not regenerate Cargo.lock."

require_version "package-lock.json" "$(jq -r .version "$PACKAGE_LOCK")"
require_version "Cargo.lock" "$(cargo_lock_version)"

# --- Run tests ---

echo "  Running Rust tests..."
(cd "$REPO_ROOT" && cargo test --quiet) || abort "Rust tests."

echo "  Building TS interface..."
(cd "$REPO_ROOT/bindings/typescript/interface" && npm run build --silent) \
  || abort "TS interface build."

echo "  Running TS parser tests..."
(cd "$REPO_ROOT/bindings/typescript/parser" && npm test --silent) || abort "TS parser tests."

echo "  All tests passed"
echo ""

# --- Commit, tag, push ---

echo "  Committing $TAG..."
git -C "$REPO_ROOT" add "${VERSION_FILES[@]}"
git -C "$REPO_ROOT" commit -m "$TAG" --quiet

echo "  Tagging $TAG..."
git -C "$REPO_ROOT" tag "$TAG"

echo "  Pushing to origin..."
if ! git -C "$REPO_ROOT" push origin main --tags --quiet; then
  echo ""
  echo "FAILED: Push to origin. Undoing local commit and tag..."
  git -C "$REPO_ROOT" tag -d "$TAG" >/dev/null 2>&1
  git -C "$REPO_ROOT" reset --hard HEAD~1 --quiet
  echo "  Reverted to pre-release state. Nothing was pushed."
  exit 1
fi

echo ""
echo "=== Released $TAG ==="
echo ""
echo "  To publish to npm, trigger the 'Publish to npm' workflow on GitHub Actions."
echo ""

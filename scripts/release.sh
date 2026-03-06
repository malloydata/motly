#!/usr/bin/env bash
set -uo pipefail

# Release process:
#   1. ./scripts/release.sh [patch|minor|major]
#   2. Trigger "Publish to npm" workflow on GitHub Actions
#
# NOTE: sed -i '' is BSD/macOS syntax. This script is meant to be run locally.

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PACKAGE_JSON="$REPO_ROOT/bindings/typescript/parser/package.json"
CARGO_TOML="$REPO_ROOT/Cargo.toml"

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
  git -C "$REPO_ROOT" checkout -- "$PACKAGE_JSON" "$CARGO_TOML" 2>/dev/null
}

sed -i '' "s/\"version\": \"$CURRENT\"/\"version\": \"$NEW\"/" "$PACKAGE_JSON"
sed -i '' "s/^version = \"$CURRENT\"/version = \"$NEW\"/" "$CARGO_TOML"

# --- Run tests ---

echo "  Running Rust tests..."
if ! (cd "$REPO_ROOT" && cargo test --quiet); then
  echo ""
  echo "FAILED: Rust tests."
  revert_version_files
  echo "  Version files reverted. Nothing was committed."
  exit 1
fi

echo "  Building TS interface..."
if ! (cd "$REPO_ROOT/bindings/typescript/interface" && npm run build --silent); then
  echo ""
  echo "FAILED: TS interface build."
  revert_version_files
  echo "  Version files reverted. Nothing was committed."
  exit 1
fi

echo "  Running TS parser tests..."
if ! (cd "$REPO_ROOT/bindings/typescript/parser" && npm test --silent); then
  echo ""
  echo "FAILED: TS parser tests."
  revert_version_files
  echo "  Version files reverted. Nothing was committed."
  exit 1
fi

echo "  All tests passed"
echo ""

# --- Commit, tag, push ---

echo "  Committing $TAG..."
git -C "$REPO_ROOT" add "$PACKAGE_JSON" "$CARGO_TOML"
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

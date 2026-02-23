#!/usr/bin/env bash
set -euo pipefail

# NOTE: This script pushes to GitHub and creates a release. It requires
# credentials that are only available in CI, not on the local CLI.
# Do NOT run this from the local machine. Instead, bump the version
# manually and build the tarball with `npm run pack`.

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PACKAGE_JSON="$REPO_ROOT/bindings/typescript/parser/package.json"
CARGO_TOML="$REPO_ROOT/Cargo.toml"

BUMP="${1:-patch}"

# Read current version from package.json
CURRENT=$(grep '"version"' "$PACKAGE_JSON" | head -1 | sed 's/.*"\([0-9]*\.[0-9]*\.[0-9]*\)".*/\1/')
IFS='.' read -r MAJOR MINOR PATCH <<< "$CURRENT"

case "$BUMP" in
  patch) PATCH=$((PATCH + 1)) ;;
  minor) MINOR=$((MINOR + 1)); PATCH=0 ;;
  major) MAJOR=$((MAJOR + 1)); MINOR=0; PATCH=0 ;;
  *) echo "Usage: $0 [patch|minor|major] (default: patch)"; exit 1 ;;
esac

NEW="$MAJOR.$MINOR.$PATCH"
TAG="v$NEW"

echo "Bumping $CURRENT -> $NEW"

# Check for clean working tree
if ! git -C "$REPO_ROOT" diff --quiet || ! git -C "$REPO_ROOT" diff --cached --quiet; then
  echo "Error: working tree is not clean. Commit or stash changes first."
  exit 1
fi

# Update versions
sed -i '' "s/\"version\": \"$CURRENT\"/\"version\": \"$NEW\"/" "$PACKAGE_JSON"
sed -i '' "s/^version = \"$CURRENT\"/version = \"$NEW\"/" "$CARGO_TOML"

# Test everything
echo "Running Rust tests..."
(cd "$REPO_ROOT" && cargo test --quiet)

echo "Building interface..."
(cd "$REPO_ROOT/bindings/typescript/interface" && npm run build --silent)

echo "Running parser tests..."
(cd "$REPO_ROOT/bindings/typescript/parser" && npm test --silent)

# Commit, tag, push
echo "Committing and tagging $TAG..."
git -C "$REPO_ROOT" add "$PACKAGE_JSON" "$CARGO_TOML"
git -C "$REPO_ROOT" commit -m "$TAG"
git -C "$REPO_ROOT" tag "$TAG"
git -C "$REPO_ROOT" push origin main --tags

# GitHub release with auto-generated changelog
echo "Creating GitHub release..."
gh release create "$TAG" --generate-notes --title "$TAG" --repo "$(git -C "$REPO_ROOT" remote get-url origin)"

echo "Done: $TAG pushed. Run 'Publish to npm' action on GitHub to publish."

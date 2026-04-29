#!/usr/bin/env bash
set -euo pipefail

VERSION="${1:-}"
if [ -z "$VERSION" ]; then
    echo "Usage: $0 <version>"
    echo "Example: $0 1.9.60"
    exit 1
fi

echo "🚀 Releasing CodeLens v$VERSION"

# Update workspace version only (crates use version.workspace = true)
sed -i '' -E "s/^version = \"[^\"]+\"/version = \"$VERSION\"/" Cargo.toml

echo "✅ Version bumped to $VERSION. Review changes, then run:"
echo "   cargo check --workspace && cargo nextest run --workspace"
echo "   git add -A && git commit -m 'chore(release): prepare v$VERSION'"
echo "   git tag -a v$VERSION -m 'Release v$VERSION'"
echo "   git push && git push origin v$VERSION"

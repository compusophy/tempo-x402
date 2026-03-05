#!/bin/bash
# Bump version across ALL files at once
# Usage: ./scripts/bump-version.sh 0.2.1

set -euo pipefail

if [ -z "$1" ]; then
    echo "Usage: $0 <new-version>"
    echo "Example: $0 0.2.1"
    exit 1
fi

NEW_VERSION="$1"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"

# Validate version format
if ! echo "$NEW_VERSION" | grep -qE '^[0-9]+\.[0-9]+\.[0-9]+$'; then
    echo "ERROR: Invalid version format. Use semantic versioning (e.g., 0.2.1)"
    exit 1
fi

echo "Bumping version to: $NEW_VERSION"
echo ""

# Get current version from core crate (source of truth)
OLD_VERSION=$(grep -m1 '^version = ' "$ROOT_DIR/crates/tempo-x402/Cargo.toml" | sed 's/version = "\(.*\)"/\1/')
echo "Current version: $OLD_VERSION"
echo ""

if [ "$OLD_VERSION" = "$NEW_VERSION" ]; then
    echo "Already at version $NEW_VERSION"
    exit 0
fi

# Update each crate's Cargo.toml
CRATES=("tempo-x402" "tempo-x402-client" "tempo-x402-server" "tempo-x402-facilitator" "tempo-x402-gateway" "tempo-x402-wallet" "tempo-x402-app" "tempo-x402-security-audit" "tempo-x402-identity" "tempo-x402-agent" "tempo-x402-soul" "tempo-x402-node")
for crate in "${CRATES[@]}"; do
    CRATE_TOML="$ROOT_DIR/crates/$crate/Cargo.toml"
    if [ -f "$CRATE_TOML" ]; then
        sed -i'' -e "s/^version = \"$OLD_VERSION\"/version = \"$NEW_VERSION\"/" "$CRATE_TOML"
        echo "✓ Updated crates/$crate/Cargo.toml"
    fi
done

# Update workspace dependency version in root Cargo.toml
if [ -f "$ROOT_DIR/Cargo.toml" ]; then
    sed -i'' -e "s/version = \"$OLD_VERSION\"/version = \"$NEW_VERSION\"/g" "$ROOT_DIR/Cargo.toml"
    echo "✓ Updated Cargo.toml workspace dependency"
fi

# Update llms.txt version at the bottom
if [ -f "$ROOT_DIR/llms.txt" ]; then
    sed -i'' -e "s/^$OLD_VERSION$/$NEW_VERSION/" "$ROOT_DIR/llms.txt"
    echo "✓ Updated llms.txt"
fi

echo ""
echo "================================"
echo "Version bumped: $OLD_VERSION -> $NEW_VERSION"
echo "================================"
echo ""
echo "Next steps:"
echo "  1. Review changes: git diff"
echo "  2. Commit: git add -A && git commit -m 'Bump version to $NEW_VERSION'"
echo "  3. Tag: git tag v$NEW_VERSION"
echo "  4. Push: git push && git push --tags"
echo "  5. Publish: cargo publish -p tempo-x402 && cargo publish -p tempo-x402-server && ..."

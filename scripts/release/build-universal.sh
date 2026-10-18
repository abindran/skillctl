#!/bin/sh
# Build skillctl for Apple silicon and Intel and join them into dist/skillctl.
# Requires rustup. Set SKILLCTL_COMMIT to record a commit in --version.
set -eu
cd "$(dirname "$0")/../.."

for target in aarch64-apple-darwin x86_64-apple-darwin; do
  rustup target add "$target"
  cargo build --release --locked --target "$target"
done

mkdir -p dist
lipo -create -output dist/skillctl \
  target/aarch64-apple-darwin/release/skillctl \
  target/x86_64-apple-darwin/release/skillctl
chmod 755 dist/skillctl
lipo -info dist/skillctl
dist/skillctl --version

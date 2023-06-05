#!/bin/bash

echo "Verifying"

version=$1
token=$CARGO_REGISTRY_TOKEN

toml set Cargo.toml package.version $version
cargo publish --dry-run --token $token
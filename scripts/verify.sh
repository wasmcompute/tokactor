#!/bin/bash

version="$1"
token=$CARGO_REGISTRY_TOKEN

toml set Cargo.toml package.version $version > output.toml 2>&1 
mv output.toml Cargo.toml
cargo publish --dry-run --allow-dirty --token $token
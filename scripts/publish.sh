#!/bin/bash

echo "Publishing"

version=$1
token=$CARGO_REGISTRY_TOKEN

toml set Cargo.toml package.version $version
cargo publish --token $token
#!/bin/bash

echo "Preparing"

version=$1

toml set Cargo.toml package.version $version
cargo build --release
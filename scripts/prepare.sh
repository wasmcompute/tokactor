#!/bin/bash

echo "Preparing"

version=$1

toml set Cargo.toml package.version $version > output.toml 2>&1 
mv output.toml Cargo.toml
cargo build --release
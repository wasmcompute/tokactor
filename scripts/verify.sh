#!/bin/bash

echo "Verifying"

token=$CARGO_REGISTRY_TOKEN

cargo publish --dry-run --token $token
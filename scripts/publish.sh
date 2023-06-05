#!/bin/bash

echo "Publishing"

token=$CARGO_REGISTRY_TOKEN

cargo publish --token $token
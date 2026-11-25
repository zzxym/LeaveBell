#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"

PLATFORM="$1"   # win | linux
ARCH="$2"
src="dist-$PLATFORM/online-$ARCH"
out="dist-offline/$PLATFORM-$ARCH"

rm -rf "$out"
mkdir -p "$out"
cp -r "$src/." "$out/"

find offline/runtime -type f \( -name '*.dll' -o -name '*.so' -o -name '*.so.*' -o -name '*.dylib' \) -exec cp {} "$out/" \;

mkdir -p "$out/melo/models/sherpa-onnx-vits-zh-ll"
find offline/model -type f -exec cp {} "$out/melo/models/sherpa-onnx-vits-zh-ll/" \;

echo "Offline payload: $(pwd)/$out"

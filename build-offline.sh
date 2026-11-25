#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"

PLATFORM="$1"   # win | linux
ARCH="$2"
case "$PLATFORM" in
  win) src="dist-native/online-$ARCH" ;;
  linux) src="dist-linux/online-$ARCH" ;;
  *) echo "未知平台: $PLATFORM"; exit 1 ;;
esac
out="dist-offline/$PLATFORM-$ARCH"

rm -rf "$out"
mkdir -p "$out"
cp -r "$src/." "$out/"

# 同目录下的 .so 可能是符号链接；解引用复制，保证 libloading 能按固定文件名打开。
find offline/runtime \( -type f -o -type l \) \( -name '*.dll' -o -name '*.so' -o -name '*.so.*' -o -name '*.dylib' \) -exec cp -L {} "$out/" \;

mkdir -p "$out/melo/models/sherpa-onnx-vits-zh-ll"
find offline/model -type f -exec cp {} "$out/melo/models/sherpa-onnx-vits-zh-ll/" \;

# 离线版默认使用本地 sherpa 语音，而不是 online 载荷的在线默认设置。
mkdir -p "$out/data"
cp "preset/data-offline.json" "$out/data/data.json"

echo "Offline payload: $(pwd)/$out"

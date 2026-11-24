#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"

PLATFORM="$1"   # win | linux
ARCH="$2"

if [ "$PLATFORM" = "win" ]; then
  src="dist-native/school-display-$ARCH"
else
  src="dist-linux/LeaveBell-$ARCH"
fi

out="dist-offline/$PLATFORM-$ARCH"
rm -rf "$out"
mkdir -p "$out"
cp -r "$src/." "$out/"

# 动态库放到 exe 同目录，libloading 直接按文件名加载
find offline/runtime -type f \( -name '*.dll' -o -name '*.so' -o -name '*.so.*' -o -name '*.dylib' \) -exec cp {} "$out/" \;

# 模型与中文规则 fst 放到代码约定的路径
mkdir -p "$out/melo/models/sherpa-onnx-vits-zh-ll"
find offline/model -type f -exec cp {} "$out/melo/models/sherpa-onnx-vits-zh-ll/" \;

if [ "$PLATFORM" = "linux" ]; then
  tar -czf "LeaveBell-Offline-Linux-$ARCH.tar.gz" -C dist-offline "$PLATFORM-$ARCH"
fi
echo "Offline payload: $(pwd)/$out"

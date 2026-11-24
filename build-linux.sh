#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"

cargo build --release

out="dist-linux/LeaveBell"
rm -rf "$out"
mkdir -p "$out/web" "$out/data"

cp target/release/LeaveBell "$out/"
cp -r web/. "$out/web/"
if [ -f dist/school-display/data/data.json ]; then
  cp dist/school-display/data/data.json "$out/data/"
else
  printf '{}' > "$out/data/data.json"
fi

# 离线 TTS 资源可选：放入 patch-melo/payload 的 melo/ 与 sherpa/*.so 才会随包发布
if [ -d patch-melo/payload/melo ]; then cp -r patch-melo/payload/melo "$out/"; fi
if compgen -G "patch-melo/payload/sherpa/*.so" > /dev/null; then
  mkdir -p "$out/sherpa"
  cp patch-melo/payload/sherpa/*.so "$out/sherpa/"
fi

tar -czf LeaveBell-Linux-x86_64.tar.gz -C dist-linux LeaveBell
echo "Output: $(pwd)/LeaveBell-Linux-x86_64.tar.gz"

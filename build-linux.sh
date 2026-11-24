#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"

TARGET="${1:-}"
ARCH="${2:-amd64}"

build_args=(build --release)
if [ -n "$TARGET" ]; then build_args+=(--target "$TARGET"); fi
cargo "${build_args[@]}"

bin="target/${TARGET:+$TARGET/}release/LeaveBell"
[ -f "$bin" ] || bin="target/release/LeaveBell"

out="dist-linux/LeaveBell-$ARCH"
rm -rf "$out"
mkdir -p "$out/web" "$out/data"

cp "$bin" "$out/LeaveBell.bin"
cat > "$out/LeaveBell" <<'EOF'
#!/usr/bin/env bash
DIR="$(cd "$(dirname "$0")" && pwd)"
export LD_LIBRARY_PATH="$DIR:${LD_LIBRARY_PATH:-}"
exec "$DIR/LeaveBell.bin" "$@"
EOF
chmod +x "$out/LeaveBell"

cp -r web/. "$out/web/"
if [ -f dist/school-display/data/data.json ]; then
  cp dist/school-display/data/data.json "$out/data/"
else
  printf '{}' > "$out/data/data.json"
fi

tar -czf "LeaveBell-Online-Linux-$ARCH.tar.gz" -C dist-linux "LeaveBell-$ARCH"
echo "Output: $(pwd)/LeaveBell-Online-Linux-$ARCH.tar.gz"

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
screen_bin="$(dirname "$bin")/LeaveBellScreen"

online="dist-linux/online-$ARCH"
screen="dist-linux/screen-$ARCH"
for out in "$online" "$screen"; do
  rm -rf "$out"
  mkdir -p "$out"
done
mkdir -p "$online/web" "$online/data"

cp "$bin" "$online/LeaveBell.bin"
cp "$screen_bin" "$online/LeaveBellScreen.bin"
cp "$screen_bin" "$screen/LeaveBellScreen.bin"
cp "assets/OFL-Noto-CJK.txt" "$online/OFL-Noto-CJK.txt"
cp "assets/OFL-Noto-CJK.txt" "$screen/OFL-Noto-CJK.txt"
cat > "$online/LeaveBell" <<'EOF'
#!/usr/bin/env bash
DIR="$(cd "$(dirname "$(readlink -f "$0")")" && pwd)"
export LD_LIBRARY_PATH="$DIR:${LD_LIBRARY_PATH:-}"
exec "$DIR/LeaveBell.bin" "$@"
EOF
cat > "$online/LeaveBellScreen" <<'EOF'
#!/usr/bin/env bash
DIR="$(cd "$(dirname "$(readlink -f "$0")")" && pwd)"
exec "$DIR/LeaveBellScreen.bin" "$@"
EOF
cat > "$screen/LeaveBellScreen" <<'EOF'
#!/usr/bin/env bash
DIR="$(cd "$(dirname "$(readlink -f "$0")")" && pwd)"
exec "$DIR/LeaveBellScreen.bin" "$@"
EOF
chmod +x "$online/LeaveBell" "$online/LeaveBellScreen" "$screen/LeaveBellScreen"

cp -r web/. "$online/web/"
cp preset/data.json "$online/data/data.json"

echo "Online payload: $(pwd)/$online"
echo "Screen payload: $(pwd)/$screen"

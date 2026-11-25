#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"

ARCH="$1"
VARIANT="${2:-offline}"
REF="${GITHUB_REF_NAME:-v2.0.0}"
case "$REF" in
  v*) VER="${REF#v}" ;;
  *) VER="2.0.0" ;;
esac

case "$VARIANT" in
  offline) SRC="dist-offline/linux-$ARCH"; OUT="LeaveBell-$ARCH" ;;
  online) SRC="dist-linux/LeaveBell-$ARCH"; OUT="LeaveBell-Online-$ARCH" ;;
  *) echo "未知版本: $VARIANT"; exit 1 ;;
esac
[ -d "$SRC" ] || { echo "缺少包源: $SRC"; exit 1; }

case "$ARCH" in
  amd64) DEB_ARCH=amd64; RPM_ARCH=x86_64 ;;
  arm64) DEB_ARCH=arm64; RPM_ARCH=aarch64 ;;
  *) echo "未知架构: $ARCH"; exit 1 ;;
esac

STAGE="dist-packages/linux-$ARCH"
rm -rf "$STAGE" rpmbuild
mkdir -p "$STAGE/opt/LeaveBell" "$STAGE/usr/bin" "$STAGE/usr/share/applications"
cp -r "$SRC/." "$STAGE/opt/LeaveBell/"
cat > "$STAGE/usr/share/applications/LeaveBell.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=LeaveBell
Exec=/usr/bin/LeaveBell
Terminal=false
Categories=Education;
EOF
ln -s /opt/LeaveBell/LeaveBell "$STAGE/usr/bin/LeaveBell"

mkdir -p "$STAGE/DEBIAN"
cat > "$STAGE/DEBIAN/control" <<EOF
Package: leavebell
Version: $VER
Section: education
Priority: optional
Architecture: $DEB_ARCH
Maintainer: School <admin@local>
Description: LeaveBell school pickup display
EOF
dpkg-deb --build "$STAGE" "$OUT.deb"

mkdir -p rpmbuild/BUILD rpmbuild/RPMS rpmbuild/SOURCES rpmbuild/SPECS rpmbuild/SRPMS
cat > rpmbuild/SPECS/LeaveBell.spec <<EOF
Name: LeaveBell
Version: $VER
Release: 1
Summary: LeaveBell school pickup display
License: Proprietary
%description
LeaveBell school pickup display.
%install
rm -rf %{buildroot}
mkdir -p %{buildroot}/opt/LeaveBell %{buildroot}/usr/bin %{buildroot}/usr/share/applications
cp -a %{_sourcedir}/$SRC/. %{buildroot}/opt/LeaveBell/
ln -s /opt/LeaveBell/LeaveBell %{buildroot}/usr/bin/LeaveBell
cat > %{buildroot}/usr/share/applications/LeaveBell.desktop <<'EOD'
[Desktop Entry]
Type=Application
Name=LeaveBell
Exec=/usr/bin/LeaveBell
Terminal=false
Categories=Education;
EOD
%files
/opt/LeaveBell
/usr/bin/LeaveBell
/usr/share/applications/LeaveBell.desktop
EOF
rpmbuild -bb --target "$RPM_ARCH" --define "_topdir $PWD/rpmbuild" --define "_sourcedir $PWD" rpmbuild/SPECS/LeaveBell.spec
cp rpmbuild/RPMS/$RPM_ARCH/LeaveBell-$VER-1.*.rpm "$OUT.rpm"

echo "Packages: $OUT.deb $OUT.rpm"

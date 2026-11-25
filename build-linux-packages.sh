#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"

ARCH="$1"
VARIANT="${2:-online}"
REF="${GITHUB_REF_NAME:-v2.0.0}"
case "$REF" in
  v*) VER="${REF#v}" ;;
  *) VER="2.0.0" ;;
esac

case "$VARIANT" in
  online)
    SRC="dist-linux/online-$ARCH"; PKG="leavebell-online"; OUT="LeaveBell-online-linux-$ARCH"
    OPT="LeaveBell-online"; MAIN="LeaveBell"; LINK="LeaveBell-online"; DESKTOP="LeaveBellOnline"; TITLE="LeaveBell Online" ;;
  offline)
    SRC="dist-offline/linux-$ARCH"; PKG="leavebell-offline"; OUT="LeaveBell-offline-linux-$ARCH"
    OPT="LeaveBell-offline"; MAIN="LeaveBell"; LINK="LeaveBell-offline"; DESKTOP="LeaveBellOffline"; TITLE="LeaveBell Offline" ;;
  screen)
    SRC="dist-linux/screen-$ARCH"; PKG="leavebell-screen"; OUT="LeaveBell-screen-linux-$ARCH"
    OPT="LeaveBell-screen"; MAIN="LeaveBellDisplay"; LINK="LeaveBellScreen"; DESKTOP="LeaveBellScreen"; TITLE="LeaveBell Screen" ;;
  *) echo "未知版本: $VARIANT"; exit 1 ;;
esac
[ -d "$SRC" ] || { echo "缺少包源: $SRC"; exit 1; }

case "$ARCH" in
  amd64) DEB_ARCH=amd64; RPM_ARCH=x86_64 ;;
  arm64) DEB_ARCH=arm64; RPM_ARCH=aarch64 ;;
  *) echo "未知架构: $ARCH"; exit 1 ;;
esac

STAGE="dist-packages/linux-$ARCH-$VARIANT"
rm -rf "$STAGE" rpmbuild
mkdir -p "$STAGE/opt/$OPT" "$STAGE/usr/bin" "$STAGE/usr/share/applications"
cp -r "$SRC/." "$STAGE/opt/$OPT/"
cat > "$STAGE/usr/share/applications/$DESKTOP.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=$TITLE
Exec=/usr/bin/$LINK
Terminal=false
Categories=Education;
EOF
ln -s "/opt/$OPT/$MAIN" "$STAGE/usr/bin/$LINK"

mkdir -p "$STAGE/DEBIAN"
cat > "$STAGE/DEBIAN/control" <<EOF
Package: $PKG
Version: $VER
Section: education
Priority: optional
Architecture: $DEB_ARCH
Maintainer: School <admin@local>
Description: $TITLE
EOF
dpkg-deb --build "$STAGE" "$OUT.deb"

mkdir -p rpmbuild/BUILD rpmbuild/RPMS rpmbuild/SOURCES rpmbuild/SPECS rpmbuild/SRPMS
cat > rpmbuild/SPECS/LeaveBell.spec <<EOF
Name: $PKG
Version: $VER
Release: 1
Summary: $TITLE
License: Proprietary
%description
$TITLE
%install
rm -rf %{buildroot}
mkdir -p %{buildroot}/opt/$OPT %{buildroot}/usr/bin %{buildroot}/usr/share/applications
cp -a %{_sourcedir}/$SRC/. %{buildroot}/opt/$OPT/
ln -s /opt/$OPT/$MAIN %{buildroot}/usr/bin/$LINK
cat > %{buildroot}/usr/share/applications/$DESKTOP.desktop <<'EOD'
[Desktop Entry]
Type=Application
Name=$TITLE
Exec=/usr/bin/$LINK
Terminal=false
Categories=Education;
EOD
%files
/opt/$OPT
/usr/bin/$LINK
/usr/share/applications/$DESKTOP.desktop
EOF
rpmbuild -bb --target "$RPM_ARCH" --define "_topdir $PWD/rpmbuild" --define "_sourcedir $PWD" rpmbuild/SPECS/LeaveBell.spec
cp rpmbuild/RPMS/$RPM_ARCH/$PKG-$VER-1.*.rpm "$OUT.rpm"

echo "Packages: $OUT.deb $OUT.rpm"

#!/bin/sh
# Gera target/deb/pulse-monitor_<versão>_<arq>.deb com dpkg-deb (sem cargo-deb).
#   packaging/build-deb.sh            compila em release e empacota
#   packaging/build-deb.sh --no-build usa target/release/pulse já compilado
set -eu
umask 022
cd "$(dirname "$0")/.."

PKG=pulse-monitor
BUILD=1
[ "${1:-}" = "--no-build" ] && BUILD=0
command -v cargo >/dev/null 2>&1 || PATH="$HOME/.cargo/bin:$PATH"
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
ARCH=$(dpkg --print-architecture)

if [ "$BUILD" = 1 ]; then cargo build --release; fi
BIN=target/release/pulse
[ -x "$BIN" ] || { echo "faltando $BIN" >&2; exit 1; }

# Versão mínima da glibc exigida pelo binário.
GLIBC=$(objdump -T "$BIN" | grep -o 'GLIBC_[0-9.]*' | sed 's/GLIBC_//' | sort -uV | tail -1)

ROOT=target/deb/root
rm -rf "${ROOT:?}"
install -d "$ROOT/DEBIAN" "$ROOT/usr/bin" "$ROOT/usr/lib/systemd/user" \
  "$ROOT/usr/share/doc/$PKG" "$ROOT/usr/share/$PKG" "$ROOT/usr/share/applications" \
  "$ROOT/usr/share/icons/hicolor/scalable/apps"

install -m 755 "$BIN" "$ROOT/usr/bin/pulse"
install -m 755 packaging/pulse-hook "$ROOT/usr/bin/pulse-hook"
install -m 755 packaging/deb/pulse-open "$ROOT/usr/bin/pulse-open"
install -m 644 packaging/deb/pulse.service "$ROOT/usr/lib/systemd/user/pulse.service"
install -m 644 packaging/deb/pulse.desktop "$ROOT/usr/share/applications/pulse-monitor.desktop"
install -m 644 assets/logo/pulse-app-icon.svg "$ROOT/usr/share/icons/hicolor/scalable/apps/pulse-monitor.svg"
install -m 644 packaging/agents.conf.example "$ROOT/usr/share/$PKG/agents.conf.example"
install -m 644 README.md "$ROOT/usr/share/doc/$PKG/README.md"
install -m 644 packaging/deb/copyright "$ROOT/usr/share/doc/$PKG/copyright"
printf '%s (%s) unstable; urgency=low\n\n  * Pacote gerado por packaging/build-deb.sh.\n\n -- JoaoVictorAAbreu-Dev <noreply@github.com>  %s\n' \
  "$PKG" "$VERSION" "$(LC_ALL=C date -R)" | gzip -9n > "$ROOT/usr/share/doc/$PKG/changelog.gz"
chmod 644 "$ROOT/usr/share/doc/$PKG/changelog.gz"
install -m 755 packaging/deb/postinst packaging/deb/prerm "$ROOT/DEBIAN/"

SIZE=$(du -sk --exclude=DEBIAN "$ROOT" | cut -f1)
cat > "$ROOT/DEBIAN/control" <<CTRL
Package: $PKG
Version: $VERSION
Architecture: $ARCH
Maintainer: JoaoVictorAAbreu-Dev <noreply@github.com>
Installed-Size: $SIZE
Depends: libc6 (>= $GLIBC), systemd
Recommends: curl, libnotify-bin, xdg-utils
Suggests: docker.io
Section: admin
Priority: optional
Homepage: https://github.com/JoaoVictorAAbreu-Dev/pulse-linux-monitor
Description: monitor do Ubuntu com painel de agentes de IA
 Pulse é um daemon leve (Rust, só biblioteca padrão) que lê /proc, /sys,
 systemctl e journalctl e serve uma interface web local em
 http://127.0.0.1:7878: CPU, memória, discos, processos, serviços, timers,
 cron, contêineres, rede, logs, alertas e os agentes de IA (Claude Code,
 Codex, OpenClaw) com sessões e eventos ao vivo via hooks.
 .
 Escuta só no loopback e exige token. O serviço é de usuário:
 systemctl --user enable --now pulse
CTRL

(cd "$ROOT" && find usr -type f -exec md5sum {} + | sort -k2) > "$ROOT/DEBIAN/md5sums"
chmod 644 "$ROOT/DEBIAN/control" "$ROOT/DEBIAN/md5sums"

OUT="target/deb/${PKG}_${VERSION}_${ARCH}.deb"
fakeroot dpkg-deb --build -Zxz "$ROOT" "$OUT" >/dev/null
echo "$OUT"

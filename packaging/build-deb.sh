#!/bin/sh
# Gera target/deb/pulse-monitor_<versão>_<arq>.deb com dpkg-deb (sem cargo-deb).
#   packaging/build-deb.sh                     compila em release (arquitetura local) e empacota
#   packaging/build-deb.sh --no-build          usa o binário já compilado
#   packaging/build-deb.sh --target aarch64-unknown-linux-gnu   empacota para arm64
set -eu
umask 022
cd "$(dirname "$0")/.."

PKG=pulse-monitor
BUILD=1
TARGET=""
while [ $# -gt 0 ]; do
  case "$1" in
    --no-build) BUILD=0 ;;
    --target) TARGET="$2"; shift ;;
    *) echo "opção desconhecida: $1" >&2; exit 2 ;;
  esac
  shift
done
command -v cargo >/dev/null 2>&1 || PATH="$HOME/.cargo/bin:$PATH"
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)

case "$TARGET" in
  "") ARCH=$(dpkg --print-architecture); BIN=target/release/pulse ;;
  x86_64-unknown-linux-gnu) ARCH=amd64; BIN=target/$TARGET/release/pulse ;;
  aarch64-unknown-linux-gnu) ARCH=arm64; BIN=target/$TARGET/release/pulse ;;
  *) echo "alvo não suportado: $TARGET" >&2; exit 2 ;;
esac

if [ "$BUILD" = 1 ]; then
  if [ -n "$TARGET" ]; then cargo build --release --target "$TARGET"; else cargo build --release; fi
fi
[ -x "$BIN" ] || { echo "faltando $BIN" >&2; exit 1; }

# Versão mínima da glibc exigida (readelf lê ELF de qualquer arquitetura).
GLIBC=$(readelf --dyn-syms -W "$BIN" | grep -o 'GLIBC_[0-9.]*' | sed 's/GLIBC_//' | sort -uV | tail -1)
[ -n "$GLIBC" ] || { echo "não foi possível detectar a glibc exigida" >&2; exit 1; }

ROOT=target/deb/root-$ARCH
rm -rf "${ROOT:?}"
install -d "$ROOT/DEBIAN" "$ROOT/usr/bin" "$ROOT/usr/lib/systemd/user" \
  "$ROOT/usr/share/doc/$PKG" "$ROOT/usr/share/$PKG" "$ROOT/usr/share/applications" \
  "$ROOT/usr/share/icons/hicolor/scalable/apps" "$ROOT/usr/share/man/man1" \
  "$ROOT/usr/share/bash-completion/completions"

install -m 755 "$BIN" "$ROOT/usr/bin/pulse"
install -m 755 packaging/pulse-hook "$ROOT/usr/bin/pulse-hook"
install -m 755 packaging/deb/pulse-open "$ROOT/usr/bin/pulse-open"
install -m 755 packaging/pulse-tray "$ROOT/usr/bin/pulse-tray"
install -m 644 packaging/deb/pulse.service "$ROOT/usr/lib/systemd/user/pulse.service"
install -m 644 packaging/deb/pulse.desktop "$ROOT/usr/share/applications/pulse-monitor.desktop"
install -m 644 packaging/deb/pulse-tray.desktop "$ROOT/usr/share/applications/pulse-tray.desktop"
install -m 644 assets/logo/pulse-app-icon.svg "$ROOT/usr/share/icons/hicolor/scalable/apps/pulse-monitor.svg"
install -m 644 packaging/agents.conf.example "$ROOT/usr/share/$PKG/agents.conf.example"
install -m 644 packaging/config.toml.example "$ROOT/usr/share/$PKG/config.toml.example"
install -m 644 packaging/deb/pulse.bash-completion "$ROOT/usr/share/bash-completion/completions/pulse"
for m in pulse pulse-hook pulse-open pulse-tray; do
  gzip -9n < "packaging/deb/$m.1" > "$ROOT/usr/share/man/man1/$m.1.gz"
  chmod 644 "$ROOT/usr/share/man/man1/$m.1.gz"
done
install -m 644 README.md "$ROOT/usr/share/doc/$PKG/README.md"
install -m 644 packaging/deb/copyright "$ROOT/usr/share/doc/$PKG/copyright"
printf '%s (%s) unstable; urgency=low\n\n  * Pacote gerado por packaging/build-deb.sh.\n\n -- JoaoVictorAAbreu-Dev <noreply@github.com>  %s\n' \
  "$PKG" "$VERSION" "$(LC_ALL=C date -R)" | gzip -9n > "$ROOT/usr/share/doc/$PKG/changelog.gz"
chmod 644 "$ROOT/usr/share/doc/$PKG/changelog.gz"
install -m 755 packaging/deb/postinst packaging/deb/postrm "$ROOT/DEBIAN/"

SIZE=$(du -sk --exclude=DEBIAN "$ROOT" | cut -f1)
cat > "$ROOT/DEBIAN/control" <<CTRL
Package: $PKG
Version: $VERSION
Architecture: $ARCH
Maintainer: JoaoVictorAAbreu-Dev <noreply@github.com>
Installed-Size: $SIZE
Depends: libc6 (>= $GLIBC), systemd, python3
Recommends: curl, libnotify-bin, python3-gi
Suggests: docker.io, xdg-utils
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

#!/bin/sh
# Instala o Pulse para o usuário atual (sem sudo).
#   ./install.sh            compila (release) e instala
#   ./install.sh --enable   também habilita e inicia o serviço systemd de usuário
#   ./install.sh --no-build usa o binário já compilado em target/release
set -eu

cd "$(dirname "$0")"
ENABLE=0
BUILD=1
for a in "$@"; do
  case "$a" in
    --enable) ENABLE=1 ;;
    --no-build) BUILD=0 ;;
    -h|--help) sed -n '2,5p' "$0"; exit 0 ;;
    *) echo "opção desconhecida: $a" >&2; exit 2 ;;
  esac
done

BIN_DIR="$HOME/.local/bin"
UNIT_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
CONF_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/pulse"
DOC_DIR="$HOME/.local/share/doc/pulse"

if [ "$BUILD" = 1 ]; then
  command -v cargo >/dev/null 2>&1 || PATH="$HOME/.cargo/bin:$PATH"
  cargo build --release
fi
[ -x target/release/pulse ] || { echo "target/release/pulse não encontrado" >&2; exit 1; }

mkdir -p "$BIN_DIR" "$UNIT_DIR" "$CONF_DIR" "$DOC_DIR"
chmod 700 "$CONF_DIR"
install -m 755 target/release/pulse "$BIN_DIR/pulse"
install -m 755 packaging/pulse-hook "$BIN_DIR/pulse-hook"
install -m 644 packaging/pulse.service "$UNIT_DIR/pulse.service"
[ -e "$CONF_DIR/agents.conf" ] || install -m 600 packaging/agents.conf.example "$CONF_DIR/agents.conf"
install -m 644 README.md "$DOC_DIR/README.md"

echo "Instalado:"
echo "  $BIN_DIR/pulse"
echo "  $BIN_DIR/pulse-hook"
echo "  $UNIT_DIR/pulse.service"

if [ "$ENABLE" = 1 ]; then
  systemctl --user daemon-reload
  systemctl --user enable --now pulse.service
  echo "Serviço habilitado: systemctl --user status pulse"
else
  echo
  echo "Próximos passos:"
  echo "  systemctl --user daemon-reload && systemctl --user enable --now pulse"
  echo "  pulse --print-token    # abre o painel com esta URL"
fi

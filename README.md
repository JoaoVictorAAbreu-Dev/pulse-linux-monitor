# Pulse

Monitor para Ubuntu com painel de agentes de IA. Um daemon em Rust (só biblioteca
padrão) lê `/proc`, `/sys`, `statvfs`, `systemctl` e `journalctl` e serve uma
interface web "Liquid Glass" em `http://127.0.0.1:7878`.

<p align="center"><img src="assets/logo/pulse-lockup.svg" alt="Pulse" width="320"></p>

## Instalação

```sh
./install.sh            # compila em release e instala em ~/.local/bin
./install.sh --enable   # também habilita o serviço systemd de usuário
pulse --print-token     # imprime a URL de acesso (com o token no fragmento #)
```

Requisitos: Rust estável (`rustup`), `curl` (para o `pulse-hook`). Sem crates externas.

## Uso

```
pulse [--port 7878] [--bind 127.0.0.1] [--allow-remote] [--interval-ms 1000]
      [--notify] [--no-agents] [--print-token] [--once] [-h]
```

| Flag | Efeito |
|---|---|
| `--port N` | porta HTTP (padrão 7878) |
| `--bind IP` | endereço de escuta; fora do loopback exige `--allow-remote` |
| `--interval-ms N` | intervalo do coletor (padrão 1000, mínimo 250) |
| `--notify` | alertas novos vão para o desktop via `notify-send` |
| `--no-agents` | desliga detecção de agentes, sessões e hooks |
| `--print-token` | imprime a URL de acesso e sai |
| `--once` | coleta duas amostras, imprime o snapshot JSON e sai |

## Telas

1. **Visão geral** — KPIs (CPU, memória, swap, disco, rede ↓/↑, temperatura), gráfico CPU/memória
   (15 min ao vivo ou 1 h / 24 h / 7 d do histórico longo), alertas, maiores consumidores,
   agentes agora e armazenamento.
2. **CPU** — histórico, usuário/sistema/iowait, carga 1/5/15, trocas de contexto, PSI, governor,
   núcleos com frequência e sensores de temperatura.
3. **Memória** — composição (apps/cache/livre), swap, shmem, páginas sujas, swappiness, mortes por OOM,
   PSI, histórico e maiores consumidores.
4. **Discos** — sistemas de arquivos (uso e inodes), dispositivos (throughput, IOPS, ocupação),
   gráfico de throughput, explorador "o que ocupa espaço?" e candidatos à limpeza
   (**apenas listados com o comando sugerido; o Pulse nunca os executa**).
5. **Processos** — tabela filtrável/ordenável; encerrar (SIGTERM → SIGKILL após 5 s), pausar/continuar e
   prioridade (renice), sempre com confirmação.
6. **Serviços** — systemd de usuário e de sistema, filtros, busca, iniciar/parar/reiniciar; timers do systemd
   (próxima/última execução), cron (crontab do usuário, `/etc/crontab`, `/etc/cron.d` e `cron.{daily,...}`,
   com variáveis ocultas e senhas/tokens mascarados) e contêineres Docker (estado, portas, CPU/memória sob
   demanda e iniciar/parar/reiniciar com confirmação e auditoria).
7. **Rede** — ↓/↑, conexões TCP por estado, interfaces e portas em escuta com processo dono e marca
   "só local" / "acessível na rede".
8. **Logs** — journalctl (sistema/usuário), unidade, prioridade, nº de linhas, busca e "seguir".
9. **Agentes** — processos de agentes (Claude Code, Codex, OpenClaw/"Ponto" e regras de
   `agents.conf`), servidores MCP filhos, sessões a partir das transcrições locais e eventos ao vivo por hooks.
10. **Alertas** — ativos, regras, histórico e status da notificação no desktop.

## API

Todas as rotas exigem autenticação, exceto a UI estática, `GET /api/health` (mínima) e `POST /api/login`.

| Rota | Descrição |
|---|---|
| `GET /api/health` | estado do daemon (sem dados do sistema) |
| `GET /api/snapshot` | última amostra completa |
| `GET /api/history` | anel de 900 pontos (15 min a 1 s), formato colunar |
| `GET /api/history/long?range=1h\|6h\|24h\|7d` | médias de 60 s (até 7 dias) |
| `GET /api/stream` | SSE: `event: snapshot` a cada amostra e `event: hook` |
| `GET /api/processes` · `POST /api/processes/{pid}/{terminate\|kill\|pause\|resume\|renice}` | processos |
| `GET /api/services?scope=user\|system` · `POST /api/services/action` | systemd |
| `GET /api/logs?scope=&unit=&priority=&lines=&q=&cursor=` | journald |
| `GET /api/timers?scope=user\|system` · `GET /api/cron` | agendamentos |
| `GET /api/containers[?stats=1]` · `POST /api/containers/action` | Docker |
| `GET /api/disk/usage?path=` · `GET /api/disk/cleanup` | espaço em disco |
| `GET /api/agents` · `GET /api/agents/hooks` · `POST /api/agents/hooks/install` · `POST /api/agents/hook` | agentes |
| `GET /api/alerts` | alertas ativos, histórico e regras |

## Segurança

- Escuta só em `127.0.0.1`; `--bind` fora do loopback exige `--allow-remote`.
- Token aleatório de 256 bits em `~/.config/pulse/token` (arquivo 0600, diretório 0700).
  Comparado em tempo constante; nunca aparece em logs nem em respostas.
- O navegador recebe um cookie `HttpOnly; SameSite=Strict`. `GET` aceita cookie ou o cabeçalho
  `X-Pulse-Token`; `POST` **exige** o cabeçalho.
- `Host` é validado (DNS rebinding) e `Origin`, quando presente, também (CSRF).
- Cabeçalhos de segurança: CSP restritiva, `X-Frame-Options: DENY`, `nosniff`, `no-referrer`.
- Ações sensíveis (sinais em processos, ações em serviços e contêineres, instalação de hooks) vão para
  `~/.local/state/pulse/audit.log` (0600). PID 1 e o próprio Pulse são protegidos; antes do SIGKILL o
  Pulse confere se o PID não foi reutilizado.
- A URL de `--print-token` leva o token no fragmento (`#token=…`), que não é enviado ao servidor;
  a UI o troca por um cookie e limpa a barra de endereço.

## Agentes de IA

- **Processos**: casam por nome do executável ou trecho da linha de comando. Regras extras em
  `~/.config/pulse/agents.conf` (veja `packaging/agents.conf.example`). O app Claude Desktop não é contado.
- **Servidores MCP**: descendentes de um agente cujo executável/script tem `mcp` no nome (shells que apenas
  mencionam "mcp" num comando são ignorados).
- **Sessões**: `~/.claude/projects/*/*.jsonl` (Claude Code) e `~/.codex/sessions/**/*.jsonl` (Codex),
  últimas 16 sessões dos últimos 3 dias. Por privacidade, só metadados são expostos — projeto, modelo,
  nomes de ferramentas, tokens e linha do tempo. **O texto de pedidos, respostas e argumentos de
  ferramentas nunca sai do daemon.**
- **Hooks**: o botão **Conectar hooks** mescla em `~/.claude/settings.json` (com backup
  `settings.json.pulse-bak`, idempotente) chamadas a `pulse-hook` para `UserPromptSubmit`/`PreToolUse`
  (executando), `Notification` (pedindo atenção), `Stop` (aguardando) e `SessionEnd` (concluído).
  **Codex**: o diálogo mostra a linha `notify = ["…/pulse-hook", "aguardando", "codex"]` para o início de
  `~/.codex/config.toml` (o Codex passa o JSON como argumento; o `pulse-hook` aceita os dois formatos).
  O Pulse não edita esse TOML sozinho. Outros agentes: `echo '{"session_id":"x"}' | pulse-hook aguardando openclaw`.
  O script nunca bloqueia o agente (tempo limite de 2 s, sai sempre com 0) e passa o token ao `curl`
  pela entrada padrão, não pela linha de comando.

## Histórico longo

`LongStore` (`pulse-core::longterm`) agrega as amostras em médias de 60 s (`bucket = ts / 60_000`,
`ts` em ms), retém 7 dias (10 080 pontos) e grava em `~/.local/state/pulse/history.log`, só-anexar.
Linhas inválidas são ignoradas ao carregar; quando o arquivo passa de 2× a capacidade, é compactado via
arquivo temporário + `rename`. Erros de E/S ficam em `/api/health` (`long_error`) e nunca derrubam o daemon.

## Estrutura

```
crates/pulse-core     coletores (/proc, /sys, statvfs, systemctl, journalctl), JSON, histórico, alertas
crates/pulse-agents   detecção de agentes, MCP, transcrições e hooks
crates/pulse-daemon   HTTP/SSE, autenticação, API e UI embutida (include_str!)
packaging/            pulse.service, pulse-hook, agents.conf.example
assets/logo/          marca, ícone de app, versões mono e lockup
```

## Validação

- `cargo test`: 62 testes (coletores com amostras sintéticas, JSON, histórico longo — agregação em ms,
  recarga, linhas inválidas, compactação, falhas de E/S —, alertas, auth, HTTP, hooks, transcrições, cron/Docker).
- Daemon real: rotas, autenticação (401/421/403), SSE, `--once`, reinício com recarga do histórico longo.
- Interface conferida em Chromium headless (1440×900, claro/escuro e 390 px): sem erros de JavaScript,
  sem tabelas/textos transbordando e sem rolagem horizontal no celular.

## Desenvolvimento

```sh
cargo build && cargo test
cargo run -p pulse-daemon -- --port 7979 --interval-ms 500
```

## O que **não** foi feito

Seja honesto com o que existe:

- **SQLite**: o histórico longo é um arquivo texto; não há banco de dados.
- **Tauri / ícone na bandeja**: só a interface web.
- **Helper polkit próprio**: ações em serviços de sistema chamam `systemctl` e dependem do agente polkit
  da sessão. **Esse fluxo não foi testado.**
- **SMART** (saúde de discos, exige root/`smartctl`) e **eBPF**: não implementados.
- **Docker**: só via CLI `docker` (precisa do usuário no grupo `docker`); sem logs de contêiner nem Podman.
- **Sessões do OpenClaw/"Ponto"**: só processos e hooks; o formato das transcrições não foi mapeado.
- **Hooks do Codex/OpenClaw**: sem instalação automática (o trecho do Codex é exibido para colar).
- Por privacidade, a tela Agentes mostra só **nomes** de ferramentas, não argumentos (o mock do Figma mostra
  comandos como `npm test`; isso foi uma decisão consciente).
- Processos de outros usuários aparecem sem dono nas portas em escuta (exigiria root).
- Sem TLS: o painel foi pensado para loopback. Com `--allow-remote`, use um túnel (SSH) ou proxy com TLS.

## Licença

MIT.

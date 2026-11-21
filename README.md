# Pulse

[![CI](https://github.com/JoaoVictorAAbreu-Dev/pulse-linux-monitor/actions/workflows/ci.yml/badge.svg)](https://github.com/JoaoVictorAAbreu-Dev/pulse-linux-monitor/actions/workflows/ci.yml)

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

### Pacote .deb (Ubuntu 24.04+ / Debian com glibc ≥ 2.39)

Baixe o `.deb` da página de Releases do GitHub (amd64 e arm64) ou gere localmente:

```sh
packaging/build-deb.sh                       # gera target/deb/pulse-monitor_<versão>_amd64.deb
packaging/build-deb.sh --target aarch64-unknown-linux-gnu   # arm64 (precisa de gcc-aarch64-linux-gnu)
sudo apt install ./target/deb/pulse-monitor_0.3.0_amd64.deb
systemctl --user daemon-reload && systemctl --user enable --now pulse
```

O pacote instala `/usr/bin/pulse`, `/usr/bin/pulse-hook`, `/usr/bin/pulse-open` (abre o painel no
navegador), man pages, autocompletar do bash, exemplos de `config.toml`/`agents.conf`, o serviço de usuário em `/usr/lib/systemd/user/pulse.service`, o atalho **Pulse** no menu de
aplicativos e o ícone. O serviço **não** é habilitado automaticamente para todos os usuários.
Se você também usou o `install.sh`, a unidade em `~/.config/systemd/user/` tem prioridade; remova-a
(e `~/.local/bin/pulse*`) para usar a versão do pacote.

## Uso

```
pulse [--port 7878] [--bind 127.0.0.1] [--allow-remote] [--interval-ms 1000]
      [--notify] [--no-agents] [--config ARQ]
pulse --once | --print-token | --rotate-token | --print-config | -h
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
| `--config ARQ` | arquivo de configuração (padrão `~/.config/pulse/config.toml`) |
| `--rotate-token` | gera um token novo; o daemon em execução o adota em até 3 s |
| `--print-config` | imprime um `config.toml` de exemplo comentado |

### Arquivo de configuração

`~/.config/pulse/config.toml` (subconjunto de TOML). Precedência: padrões < arquivo < flags.

```toml
port = 7878
interval_ms = 1000
notify = false

[history]
retention_days = 7      # 1 a 90 dias de médias de 60 s

[alerts]
cpu_pct = 90
cpu_samples = 30
mem_warn = 90
mem_crit = 96
disk_warn = 90
disk_crit = 97
temp_crit = 88
```

Os limiares também podem ser editados na tela **Alertas** ("Editar limiares"): o Pulse reescreve só a
seção `[alerts]`, preservando comentários e o resto do arquivo, e a mudança vale na hora.
Linhas inválidas viram avisos (no log do serviço e em `GET /api/config`) e não impedem o daemon de subir.

## Telas

1. **Visão geral** — KPIs (CPU, memória, swap, disco, rede ↓/↑, temperatura), gráfico CPU/memória
   (15 min ao vivo ou 1 h / 24 h / 7 d em médias de 1 min, 30 d / 90 d em médias de 15 min), alertas, maiores consumidores,
   agentes agora e armazenamento.
2. **CPU** — histórico, usuário/sistema/iowait, carga 1/5/15, trocas de contexto, PSI, governor,
   núcleos com frequência e sensores de temperatura; **GPU** (uso, VRAM/GTT, frequência, temperatura e
   potência via amdgpu/i915 no `/sys/class/drm`, ou `nvidia-smi`); **energia** (baterias com saúde e
   tempo restante; consumo da CPU via RAPL quando legível).
3. **Memória** — composição (apps/cache/livre), swap, shmem, páginas sujas, swappiness, mortes por OOM,
   PSI, histórico, maiores consumidores e **quem está na swap** (`VmSwap` por processo).
4. **Discos** — sistemas de arquivos (uso e inodes), dispositivos (throughput, IOPS, ocupação),
   gráfico de throughput, **saúde SMART** opcional (via `smartctl`, sob demanda; o número de série nunca
   é exposto), explorador "o que ocupa espaço?" **em segundo plano** (com progresso, cache de 5 min e
   prioridade baixa) e candidatos à limpeza
   (**apenas listados com o comando sugerido; o Pulse nunca os executa**).
5. **Processos** — tabela filtrável/ordenável com **E/S de disco por processo** (`/proc/PID/io`) e
   **visão em árvore** (CPU e RSS somados por subárvore); encerrar (SIGTERM → SIGKILL após 5 s), pausar/continuar e
   prioridade (renice), sempre com confirmação.
6. **Serviços** — systemd de usuário e de sistema, filtros, busca, iniciar/parar/reiniciar; timers do systemd
   (próxima/última execução), **CPU e memória por unidade** a partir do cgroup v2 (sem root),
   botão **Logs** que abre a tela Logs filtrada pela unidade, cron (crontab do usuário, `/etc/crontab`, `/etc/cron.d` e `cron.{daily,...}`,
   com variáveis ocultas e senhas/tokens mascarados) e contêineres Docker (estado, portas, CPU/memória sob
   demanda — ou pelo cgroup, sem `docker stats` —, **logs do contêiner** e iniciar/parar/reiniciar com
   confirmação e auditoria).
7. **Rede** — ↓/↑, conexões TCP por estado, interfaces e portas em escuta com processo dono e marca
   "só local" / "acessível na rede".
8. **Logs** — journalctl (sistema/usuário), unidade, prioridade, nº de linhas, busca e "seguir".
9. **Agentes** — processos de agentes (Claude Code, Codex, OpenClaw/"Ponto" e regras de
   `agents.conf`), servidores MCP filhos, sessões (Claude Code, Codex e OpenClaw) com **ritmo** (tempo ativo
   e tempo esperando você), **latência por ferramenta** e **custo estimado**, **uso diário de tokens em 30
   dias** por agente e modelo, e eventos ao vivo por hooks (subagentes, falhas de ferramenta, pedidos de
   permissão; Codex com "executando" inferido pela transcrição).
10. **Alertas** — ativos, regras, histórico e status da notificação no desktop.

## API

Todas as rotas exigem autenticação, exceto a UI estática, `GET /api/health` (mínima) e `POST /api/login`.

| Rota | Descrição |
|---|---|
| `GET /api/health` | estado do daemon (sem dados do sistema) |
| `GET /api/snapshot` | última amostra completa |
| `GET /api/history` | anel de 900 pontos (15 min a 1 s), formato colunar |
| `GET /api/history/long?range=1h\|6h\|24h\|7d\|30d\|90d` | médias de 60 s (até 7 dias) |
| `GET /api/stream` | SSE: `event: snapshot` a cada amostra e `event: hook` |
| `GET /api/processes` · `POST /api/processes/{pid}/{terminate\|kill\|pause\|resume\|renice}` | processos |
| `GET /api/services?scope=user\|system` · `POST /api/services/action` | systemd |
| `GET /api/logs?scope=&unit=&priority=&lines=&q=&cursor=` | journald |
| `GET /api/timers?scope=user\|system` · `GET /api/cron` | agendamentos |
| `GET /api/cgroups` | CPU/memória por unidade systemd e contêiner (cgroup v2) |
| `GET /api/disk/smart` | saúde dos discos via `smartctl` (opcional) |
| `GET /api/disk/usage?path=&refresh=1` | explorador de espaço em segundo plano (`pending` + progresso, depois o resultado) |
| `GET /api/containers/logs?id=&lines=` | últimas linhas de log de um contêiner |
| `GET /api/containers[?stats=1]` · `POST /api/containers/action` | Docker |
| `GET /api/disk/usage?path=` · `GET /api/disk/cleanup` | espaço em disco |
| `GET /api/agents` · `GET /api/agents/hooks` · `POST /api/agents/hooks/install` · `POST /api/agents/hook` | agentes |
| `GET /api/alerts` | alertas ativos, histórico e regras |
| `GET /api/config` · `POST /api/config/alerts` | configuração efetiva e edição dos limiares |

## Segurança

- Escuta só em `127.0.0.1`; `--bind` fora do loopback exige `--allow-remote`.
- Token aleatório de 256 bits em `~/.config/pulse/token` (arquivo 0600, diretório 0700).
  Comparado em tempo constante; nunca aparece em logs nem em respostas.
- O navegador recebe um cookie `HttpOnly; SameSite=Strict`. `GET` aceita cookie ou o cabeçalho
  `X-Pulse-Token`; `POST` **exige** o cabeçalho.
- `Host` é validado (DNS rebinding) e `Origin`, quando presente, também (CSRF).
- Após 10 tentativas com token errado em 10 minutos, o IP fica bloqueado por 5 minutos (HTTP 429,
  registrado como `auth.block` na auditoria); cada falha também atrasa a resposta progressivamente.
- `pulse --rotate-token` troca o token sem reiniciar o serviço (cookies antigos deixam de valer).
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
  `settings.json.pulse-bak`, idempotente; reinstalar só acrescenta os eventos que faltam) chamadas a
  `pulse-hook` para `UserPromptSubmit`/`PreToolUse` (executando), `PermissionRequest`/`Notification`
  (pedindo atenção), `PostToolUseFailure` (falha de ferramenta), `SubagentStart`/`SubagentStop`
  (subagentes), `Stop` (aguardando) e `SessionEnd` (concluído).
- **Aviso de atenção**: quando um agente pede atenção, o Pulse avisa no desktop (`notify-send`), no máximo
  1 vez por minuto por sessão. Desligue em **Preferências** ou com `[agents] notify_attention = false`.
- **Custo**: cadastre preços (US$ por milhão de tokens: entrada, saída, leitura e escrita de cache) em
  **Preferências** ou em `[prices]` no `config.toml`. O Pulse **não traz preços prontos** — confira os
  valores atuais no site de cada provedor. Um modelo casa pelo nome exato ou pelo prefixo mais longo.
  O uso diário vem de um índice das transcrições dos últimos 30 dias, reconstruído a cada 5 min (só
  arquivos alterados são relidos).
- **Modo detalhado (opt-in)**: `[agents] show_tool_details = true` mostra comando, caminho ou padrão de
  cada ferramenta na linha do tempo e nos eventos ao vivo, com valores que parecem senhas/tokens
  mascarados. Conteúdo de arquivos, patches e o texto das conversas continuam fora.
- **OpenClaw**: as sessões ficam em SQLite (`~/.openclaw/agents/*/agent/openclaw-agent.sqlite`) e são lidas
  em modo somente leitura pelo programa `sqlite3`, se instalado (`sudo apt install sqlite3`); só janela da
  sessão, contagem de mensagens e modelo.
  **Codex**: o diálogo mostra a linha `notify = ["…/pulse-hook", "aguardando", "codex"]` para o início de
  `~/.codex/config.toml` (o Codex passa o JSON como argumento; o `pulse-hook` aceita os dois formatos).
  O Pulse não edita esse TOML sozinho. Outros agentes: `echo '{"session_id":"x"}' | pulse-hook aguardando openclaw`.
  O script nunca bloqueia o agente (tempo limite de 2 s, sai sempre com 0) e passa o token ao `curl`
  pela entrada padrão, não pela linha de comando.

## Histórico longo

`LongStore` (`pulse-core::longterm`) agrega as amostras em médias de 60 s (`bucket = ts / 60_000`,
`ts` em ms), retém 7 dias (10 080 pontos) e grava em `~/.local/state/pulse/history.log`, só-anexar.
A retenção é configurável (`[history] retention_days`, até 90 dias). Uma segunda camada em
`history-15m.log` guarda médias de 15 min por 90 dias (preenchida a partir da camada de 1 min na primeira
execução) e atende às faixas de 30 e 90 dias. Linhas inválidas são ignoradas ao carregar; quando o arquivo passa de 2× a capacidade, é compactado via
arquivo temporário + `rename`. Erros de E/S ficam em `/api/health` (`long_error`) e nunca derrubam o daemon.

## Estrutura

```
crates/pulse-core     coletores (/proc, /sys, statvfs, systemctl, journalctl), JSON, histórico, alertas
crates/pulse-agents   detecção de agentes, MCP, transcrições e hooks
crates/pulse-daemon   HTTP/SSE, autenticação, API e UI embutida (include_str!)
packaging/            pulse.service, pulse-hook, agents.conf.example, build-deb.sh, deb/
assets/logo/          marca, ícone de app, versões mono e lockup
```

Eventos de hooks e o histórico de alertas também persistem (`hooks.log` e `alerts.log` em
`~/.local/state/pulse/`, últimos 300 de cada, mesmo esquema só-anexar com compactação) e são recarregados
ao reiniciar o serviço.

## Validação

- CI (GitHub Actions): `cargo fmt --check`, `clippy -D warnings`, testes, sintaxe da UI e dos scripts;
  depois gera os `.deb` amd64 e arm64, roda `lintian --fail-on warning` e instala/remove o pacote amd64.
  Em tags `v*`, publica uma Release com os `.deb` e `SHA256SUMS`.
- `cargo test`: 72 testes unitários + 9 de integração (`crates/pulse-daemon/tests/api.rs`), que sobem o
  binário real com HOME/XDG temporários e cobrem autenticação, Host/Origin, SSE, persistência entre
  reinícios, arquivo de configuração, rotação do token, bloqueio por força bruta e a CLI.
- Antes da Fase 1: 62 testes (coletores com amostras sintéticas, JSON, histórico longo — agregação em ms,
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
- **SMART**: implementado via `smartctl`, mas **não testado em hardware real** (o smartmontools não
  está instalado na máquina de desenvolvimento; só o parser foi testado com saídas de exemplo). Ler SMART
  normalmente exige root ou o grupo `disk`.
- **eBPF**: não implementado.
- **RAPL**: o consumo da CPU só aparece se `/sys/class/powercap/intel-rapl:0/energy_uj` for legível;
  em kernels recentes ele é restrito ao root. Bateria só testada com dados de exemplo (máquina desktop).
- **Docker**: só via CLI `docker` (precisa do usuário no grupo `docker`); sem logs de contêiner nem Podman.
- **Sessões do OpenClaw/"Ponto"**: leitura implementada via `sqlite3`, mas o programa não está instalado na
  máquina de desenvolvimento; a consulta SQL foi validada contra os bancos reais (pelo módulo sqlite3 do
  Python) e o parser com saídas de exemplo.
- **Hooks do Codex/OpenClaw**: sem instalação automática (o trecho do Codex é exibido para colar).
- Por privacidade, a tela Agentes mostra só **nomes** de ferramentas, não argumentos (o mock do Figma mostra
  comandos como `npm test`; isso foi uma decisão consciente).
- Processos de outros usuários aparecem sem dono nas portas em escuta (exigiria root).
- Sem TLS: o painel foi pensado para loopback. Com `--allow-remote`, use um túnel (SSH) ou proxy com TLS.

## Licença

MIT.

# Roadmap do Pulse

Plano de melhorias e novas features, ordenado por valor e esforço. Cada item diz **por que** importa,
**o que** fazer e **como validar**. Esforço: P (≤ 1 dia), M (2–4 dias), G (1–2 semanas).

Princípios que continuam valendo: só biblioteca padrão no daemon (dependências opcionais ficam atrás de
comandos externos), loopback + token por padrão, nada destrutivo sem confirmação e auditoria, e nunca
expor conteúdo de conversas dos agentes.

---

## Fase 1 — Confiabilidade e distribuição

**Status (21/11/2026):** 1.1–1.6 implementados e validados localmente (fmt, clippy `-D warnings`,
81 testes, lintian sem avisos, instalação/remoção do `.deb` num contêiner Ubuntu 24.04).
O workflow de CI ainda precisa rodar no GitHub para ser considerado validado lá.
1.7 pendente (exige chave GPG e GitHub Pages — decisão do mantenedor).

| # | Item | Esforço | Por quê / o quê | Validação |
|---|---|---|---|---|
| 1.1 | **CI no GitHub Actions** | P | Hoje nada impede um push quebrado. Workflow com `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test` e `node --check` no `app.js`; em tags `v*`, rodar `packaging/build-deb.sh` e anexar o `.deb` ao Release. | PR com falha proposital bloqueia; tag gera Release com `.deb`. |
| 1.2 | **Testes de integração HTTP** | M | Os 62 testes cobrem módulos isolados; a API foi validada só manualmente. Subir o daemon numa porta efêmera com `XDG_*` temporários e testar 401/421/403, POST sem cabeçalho, SSE, `--once` e reinício com histórico longo. | `cargo test --test api` verde no CI. |
| 1.3 | **Persistir eventos de hooks e histórico de alertas** | P | Hoje somem a cada reinício do serviço. Arquivo só-anexar em `~/.local/state/pulse/` (mesmo padrão do `LongStore`: rotação por tamanho, linhas inválidas ignoradas). | Reiniciar o serviço mantém "Ao vivo" e o histórico de Alertas. |
| 1.4 | **Arquivo de configuração** `~/.config/pulse/config.toml` (subset INI/TOML simples) | M | Limiares de alerta, porta, intervalo e retenção hoje são fixos no código. Expor também `GET/POST /api/config` com validação, e editar regras na tela Alertas. | Mudar `cpu_pct = 80` dispara alerta a 80%; config inválida é rejeitada sem derrubar o daemon. |
| 1.5 | **Proteção contra força bruta do token** | P | Atraso progressivo e bloqueio temporário após N falhas por IP; registrar no `audit.log`. `pulse --rotate-token` para trocar o token (invalida cookies). | Teste de integração com 20 tokens errados. |
| 1.6 | **`.deb` mais completo** | P | Hoje: gerado por `packaging/build-deb.sh` e validado com `dpkg-deb`. Faltam: `lintian` no CI, man page `pulse(1)`, completions de bash, `postrm` que remove a unidade em `purge` e uma variante `arm64` (cross no CI). | `lintian` sem erros; `apt install ./pulse-monitor_*.deb` e `apt purge` limpos numa VM/contêiner Ubuntu. |
| 1.7 | **Repositório APT ou PPA** | M | Atualizações via `apt upgrade` em vez de baixar `.deb` à mão. Opção simples: repositório APT assinado no GitHub Pages gerado pelo CI. | `apt update && apt install pulse-monitor` numa máquina limpa. |

## Fase 2 — Monitoramento mais profundo

**Status (21/11/2026):** 2.1–2.10 implementados e validados localmente (clippy `-D warnings`, 96 testes,
capturas das telas sem transbordos). Validado com dados reais desta máquina: GPU AMD (uso, VRAM, GTT,
temperatura, frequência), E/S de 161 processos, 99 unidades de cgroup (7 contêineres), swap por processo,
logs de contêiner, `du` assíncrono e camada de 15 min. **Não validado com hardware real:** SMART (sem
smartmontools), bateria (desktop), RAPL (restrito ao root) e NVIDIA. Custo medido: ≈ 3,8–4,2% de CPU,
contra 3,5–3,6% da v0.1.0, graças ao cache de cmdline/VmSwap.

| # | Item | Esforço | Por quê / o quê |
|---|---|---|---|
| 2.1 | **GPU** | P | Esta máquina expõe `gpu_busy_percent` e `mem_info_vram_*` (amdgpu) em `/sys/class/drm`. Novo cartão em CPU/Visão geral com uso, VRAM e temperatura (o sensor `amdgpu` já é lido). NVIDIA via `nvidia-smi` se existir. |
| 2.2 | **E/S por processo** | P | `/proc/<pid>/io` (`read_bytes`/`write_bytes`) → colunas "Disco L/E" na tabela de Processos (o Figma já prevê essa coluna). |
| 2.3 | **Árvore de processos** | M | Alternar tabela ↔ árvore (ppid), com CPU/RAM somadas por subárvore; útil para entender agentes e seus filhos. |
| 2.4 | **Uso por cgroup/serviço** | M | `/sys/fs/cgroup/**/{cpu.stat,memory.current}` mostra CPU/RAM por unidade systemd e por contêiner sem precisar do Docker. |
| 2.5 | **Pressão e swap mais acionáveis** | P | Nesta máquina a swap passou de 60–70% durante a validação. Mostrar "quem está em swap" (`VmSwap` de `/proc/<pid>/status`) e sugerir ações (só listadas). |
| 2.6 | **Explorador de espaço em segundo plano** | M | O `du` atual para em 4 s e marca "varredura parcial" na pasta pessoal. Varredura incremental em thread própria com cache por diretório e progresso via SSE. |
| 2.7 | **SMART opcional** | P | Se `smartctl` existir e houver permissão (grupo `disk` ou regra polkit), mostrar saúde, horas ligadas e setores realocados; senão, explicar como habilitar. |
| 2.8 | **Bateria e energia** | P | `/sys/class/power_supply` (notebooks) e RAPL (`/sys/class/powercap`) para consumo estimado da CPU. |
| 2.9 | **Logs de contêiner e de unidade** | P | Botão "ver logs" em Serviços/Contêineres que abre a tela Logs filtrada (`journalctl -u` / `docker logs --tail`). |
| 2.10 | **Histórico longo em camadas** | M | Além de 7 dias a 60 s, manter 90 dias a 15 min (downsampling na compactação). Faixas "30 d" e "90 d" no gráfico. |

## Fase 3 — Agentes de IA (o diferencial do projeto)

**Status (21/11/2026):** 3.1–3.7 implementados e validados localmente (clippy `-D warnings`, 107 testes,
capturas sem transbordos). Com dados reais: índice de uso de 30 dias (Claude Code e 4 modelos do Codex),
ritmo e latência das ferramentas por sessão, subagentes/falhas/pedidos de permissão via hooks, modo
detalhado mascarando tokens e preferências gravadas no `config.toml`. Custos dependem dos preços que o
usuário cadastra (nenhum preço vem embutido). **Não validado de ponta a ponta:** leitura do OpenClaw (falta
o programa `sqlite3`; consulta validada no banco real por outro meio) e os novos eventos de hook vindos do
Claude Code real (testados com payloads de exemplo).

| # | Item | Esforço | Por quê / o quê |
|---|---|---|---|
| 3.1 | **Mais estados via hooks** | P | O `settings.json` já tem `PermissionRequest`, `SubagentStart/Stop` e `PostToolUseFailure`. Mapear: pedido de permissão → "pedindo atenção"; falha de ferramenta → selo de erro; subagentes como sessões filhas. |
| 3.2 | **Notificação quando um agente pede atenção** | P | Reusar `--notify` (notify-send) para "Claude Code está esperando você", com antispam por sessão. É o caso de uso mais pedido de painéis de agentes. |
| 3.3 | **Custo estimado por sessão/dia** | M | Tabela de preços configurável por modelo × tokens já contados (entrada, saída, cache). Gráfico de tokens/custo por dia no histórico longo. |
| 3.4 | **Duração e ritmo das sessões** | P | Tempo ativo, tempo esperando o usuário (entre `Stop` e o próximo prompt) e ferramentas mais lentas (entre `PreToolUse` e `PostToolUse`). |
| 3.5 | **Sessões do OpenClaw/"Ponto"** | M | Mapear o formato quando houver transcrições; enquanto isso, `pulse-hook` a partir das notificações do OpenClaw. |
| 3.6 | **Codex com mais estados** | P | O `notify` do Codex só avisa no fim do turno. Inferir "executando" pela atividade do processo/rollout (mtime do JSONL) para não ficar sempre "aguardando". |
| 3.7 | **Modo detalhado opt-in** | P | Mostrar argumentos de ferramentas (ex.: comando do Bash) só se o usuário ativar `show_tool_details = true`, com aviso claro; padrão continua só nomes. |

## Fase 4 — Experiência e integração

**Status (21/11/2026):** 4.1, 4.2, 4.3, 4.4 e 4.6 implementados e validados localmente (clippy
`-D warnings`, 12 testes de integração, lintian sem avisos, instalação no Ubuntu 24.04). `/metrics` com 252
séries no formato do Prometheus; ntfy e webhook entregues a um receptor HTTP local; o Chromium não aponta
erros de instalabilidade da PWA; `pulse-tray` registrado na bandeja do GNOME e com menu via D-Bus;
comparação "hoje × ontem" desenhada. De bônus, o alocador do glibc foi ajustado (`mallopt`) e o RSS do
daemon caiu de ~90 MB para ~6 MB. **Pendente:** 4.5 (inglês), e Telegram/Discord reais não testados.
O `pulse-tray` usa Python + D-Bus porque o GTK3 aborta neste GNOME 50 (esquema de configurações
incompatível).

| # | Item | Esforço | Por quê / o quê |
|---|---|---|---|
| 4.1 | **Endpoint `/metrics` (Prometheus)** | P | Exportar CPU, memória, discos, rede, alertas e tokens por agente para quem já usa Grafana. |
| 4.2 | **Webhooks/ntfy para alertas** | P | Além do desktop, enviar para ntfy.sh/Discord/Telegram (URL no config, segredo fora do log). |
| 4.3 | **PWA + atalhos de teclado** | P | Manifest e ícones para "instalar" o painel como app; `g c` = CPU, `/` = busca, etc. |
| 4.4 | **Bandeja do sistema** | G | Indicador GNOME (AppIndicator via `pulse-tray` separado) com CPU/memória e estado dos agentes; sem trazer GTK para o daemon. |
| 4.5 | **Interface em inglês** | M | Strings da UI num dicionário; `pt-BR` continua padrão. Amplia o alcance do repositório público. |
| 4.6 | **Comparar períodos** | M | Sobrepor "hoje × ontem" no gráfico do histórico longo; útil para achar regressões de consumo. |

## Fase 5 — Exploratório

**Status (21/11/2026):** 5.1, 5.2 e 5.3 implementados; 5.4 decidido (não usar). Validado localmente
com clippy `-D warnings`, testes unitários e de integração (incluindo um teste com dois daemons para o
repasse), lintian sem avisos, interface conferida em Chromium headless e testes manuais em contêiner
(túnel SSH com queda/reconexão; helper e bpftrace como root). Falta conferir o fluxo pkexec → helper com
o pacote instalado.

- **5.1 Helper polkit** — `pulse-helper` em `/usr/libexec/pulse-monitor/` com a política
  `io.github.pulse-monitor.policy` (ações `manage-units` e `netprobe`, `auth_admin_keep`). O daemon chama
  `pkexec pulse-helper unit <verbo> <unidade>`; erros do pkexec viram mensagens claras (cancelado, sem
  autorização, sem agente).
- **5.2 Multi-máquina** — `[remote.NOME]` com `ssh` (túnel `ssh -L` supervisionado) ou `url` (túnel
  próprio). Repasse somente leitura `GET /api/remote/NOME/api/…` (inclui SSE) e seletor de máquina na UI.
- **5.3 eBPF sob demanda** — `pulse-helper netprobe N` roda um script bpftrace fixo por N segundos e o
  daemon resume bytes por processo e latência de disco (`POST /api/probe/net`, painel na tela Rede).
  Optamos por medição pontual em vez de um coletor contínuo com capacidades: nada roda como root o tempo
  todo e não há binário com `CAP_BPF` instalado.
- **5.4 SQLite — decisão: não usar.** As consultas do histórico são faixas de tempo sobre médias
  (1 min e 15 min) que cabem em memória (≈ 19 mil pontos); arquivos só-anexar com compactação já dão
  recarga em milissegundos e não trazem dependência nativa. Reavaliar se surgirem consultas ad hoc
  (ex.: filtros por processo ao longo de semanas).

Ideias originais:

- **Helper polkit próprio** para ações de sistema sem depender do agente da sessão (e finalmente testar esse fluxo numa VM).
- **Multi-máquina**: um Pulse "central" agregando outros via túnel SSH (nunca expor sem TLS).
- **eBPF opcional** (binário separado com capacidades) para tráfego de rede por processo e latência de disco.
- **SQLite** só se as consultas do histórico ficarem complexas demais para arquivos texto.

---

## Próximos passos sugeridos

1. 1.1 CI + 1.6 lintian/man page → todo push validado e `.deb` publicado em cada release.
2. 1.3 persistência + 3.1/3.2 estados e notificação de agentes → maior ganho percebido no dia a dia.
3. 2.1 GPU + 2.2 E/S por processo → completa as telas já desenhadas no Figma.

//! Agentes de IA no Pulse: detecção de processos (Claude Code, Codex,
//! OpenClaw/"Ponto" e regras de `agents.conf`), servidores MCP, sessões a
//! partir de transcrições locais e eventos ao vivo via hooks.

pub mod detail;
pub mod detect;
pub mod hooks;
pub mod openclaw;
pub mod sessions;
pub mod usage;

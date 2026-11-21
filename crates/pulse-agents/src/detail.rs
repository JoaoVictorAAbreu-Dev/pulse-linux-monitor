//! Resumo curto de uma chamada de ferramenta, para o modo detalhado (opt-in).
//! Só extrai campos de "endereço" (comando, caminho, padrão, URL, descrição),
//! nunca conteúdo de arquivos, patches ou mensagens; passa pela máscara de
//! segredos e é truncado.

use pulse_core::json::Value;
use pulse_core::schedule::redact;

const MAX: usize = 100;

fn one_line(s: &str) -> String {
    let flat: String = s
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let flat = flat.split_whitespace().collect::<Vec<_>>().join(" ");
    let r = redact(&flat);
    if r.chars().count() > MAX {
        format!("{}…", r.chars().take(MAX).collect::<String>())
    } else {
        r
    }
}

fn str_or_array(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::Str(s) => Some(s.clone()),
        Value::Arr(a) => {
            let parts: Vec<&str> = a.iter().filter_map(|x| x.as_str()).collect();
            if parts.is_empty() {
                None
            } else {
                Some(parts.join(" "))
            }
        }
        _ => None,
    }
}

/// Arquivos citados num patch do Codex (`*** Update File: caminho`), sem o conteúdo.
fn patch_files(p: &str) -> Option<String> {
    let files: Vec<&str> = p
        .lines()
        .filter_map(|l| {
            l.strip_prefix("*** Update File: ")
                .or_else(|| l.strip_prefix("*** Add File: "))
                .or_else(|| l.strip_prefix("*** Delete File: "))
        })
        .collect();
    if files.is_empty() {
        None
    } else {
        Some(files.join(", "))
    }
}

pub fn tool_summary(name: &str, input: &Value) -> String {
    let g = |k: &str| str_or_array(input.get(k));
    let raw = match name {
        "Bash" | "shell" | "exec_command" | "local_shell" | "container.exec" => {
            g("command").or_else(|| g("cmd"))
        }
        "Read" | "Edit" | "Write" | "MultiEdit" | "NotebookEdit" => {
            g("file_path").or_else(|| g("notebook_path"))
        }
        "Grep" | "Glob" => g("pattern"),
        "WebFetch" => g("url"),
        "WebSearch" => g("query"),
        "Task" | "Agent" => g("description"),
        "apply_patch" => g("patch")
            .or_else(|| g("input"))
            .and_then(|p| patch_files(&p)),
        _ => [
            "command",
            "file_path",
            "path",
            "pattern",
            "query",
            "url",
            "description",
        ]
        .iter()
        .find_map(|k| g(k)),
    };
    raw.map(|r| one_line(&r)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pulse_core::json;

    fn v(s: &str) -> Value {
        json::parse(s).unwrap()
    }

    #[test]
    fn summaries() {
        assert_eq!(
            tool_summary("Bash", &v(r#"{"command":"ls -la\n  /tmp"}"#)),
            "ls -la /tmp"
        );
        assert_eq!(
            tool_summary(
                "Bash",
                &v(r#"{"command":"curl -H x --password hunter2 https://u:p@h/x"}"#)
            ),
            "curl -H x --password *** https://***@h/x"
        );
        assert_eq!(
            tool_summary("shell", &v(r#"{"command":["bash","-lc","ls"]}"#)),
            "bash -lc ls"
        );
        assert_eq!(
            tool_summary(
                "Write",
                &v(r#"{"file_path":"/a/b.rs","content":"SEGREDO"}"#)
            ),
            "/a/b.rs"
        );
        assert_eq!(
            tool_summary(
                "apply_patch",
                &v(
                    r#"{"patch":"*** Begin Patch\n*** Update File: src/a.rs\n+SEGREDO\n*** Add File: b.txt\n"}"#
                )
            ),
            "src/a.rs, b.txt"
        );
        assert_eq!(tool_summary("Desconhecida", &v(r#"{"x":1}"#)), "");
        let long = format!(r#"{{"command":"{}"}}"#, "a".repeat(300));
        assert!(tool_summary("Bash", &v(&long)).chars().count() <= MAX + 1);
    }
}

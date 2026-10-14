use anyhow::{Result, ensure};

pub fn init(cmd: &str, bind: &str, limit: u16) -> Result<String> {
    ensure!(
        !cmd.is_empty()
            && cmd.as_bytes()[0].is_ascii_alphabetic()
            && cmd.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'),
        "--cmd must be a shell identifier such as zs"
    );
    ensure!(
        ![
            "cd",
            "exit",
            "eval",
            "exec",
            "source",
            "zurf",
            "command",
            "local",
            "builtin",
            "zle",
            "bindkey",
            "return",
            "function",
            "if",
            "then",
            "else",
            "elif",
            "fi",
            "for",
            "while",
            "until",
            "do",
            "done",
            "case",
            "esac",
            "select",
            "repeat",
            "time",
            "coproc",
            "in",
            "noglob",
            "nocorrect",
        ]
        .contains(&cmd),
        "choose a command name other than {cmd}"
    );
    let key = if bind == "none" {
        None
    } else {
        let letter = bind
            .strip_prefix("ctrl-")
            .filter(|s| s.len() == 1 && s.as_bytes()[0].is_ascii_alphabetic())
            .ok_or_else(|| anyhow::anyhow!("--bind must be ctrl-<letter> or none"))?;
        Some(format!("^{}", letter.to_ascii_uppercase()))
    };
    let mut script = format!(
        r#"export ZURF_LIMIT={limit}

function {cmd}() {{
    local destination
    destination="$(command zurf pick "$@")" || return $?
    [[ -n "$destination" ]] || return 1
    builtin cd -- "$destination"
}}

function _zurf_widget() {{
    local destination
    zle -I
    if destination="$(command zurf pick </dev/tty)" && [[ -n "$destination" ]]; then
        builtin cd -- "$destination"
    fi
    zle reset-prompt
    zle -R
}}
zle -N _zurf_widget
"#
    );
    if let Some(key) = key {
        script.push_str(&format!(
            "bindkey -M emacs '{key}' _zurf_widget\nbindkey -M viins '{key}' _zurf_widget\n"
        ));
    }
    Ok(script)
}

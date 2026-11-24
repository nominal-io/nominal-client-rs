//! Reports which AI coding agent, if any, is running the process.
//!
//! Every request sends an [`AI_AGENT_HEADER`] header naming the agent, or `none` when no agent
//! marks the process, so Nominal can tell agent-driven use apart from other use and from releases
//! that never checked. Detection matches the Nominal Python client.

use std::ffi::OsString;

use http::HeaderValue;

/// Request header carrying the detected AI coding agent, or [`NO_AI_AGENT`].
pub const AI_AGENT_HEADER: &str = "x-nominal-ai-agent";

/// [`AI_AGENT_HEADER`] value when no agent marks the process.
pub const NO_AI_AGENT: &str = "none";

/// Variables AI coding agents set in the commands they run, with the agent each names. Drawn from
/// @vercel/detect-agent, laravel/agent-detector, and each agent's own source; Cursor's and
/// Codex's were checked against current releases. Kilo also sets `OPENCODE`, so it comes first.
const AGENT_ENV_VARS: &[(&str, &str)] = &[
    ("KILO_PID", "kilo-code"),
    ("CLAUDECODE", "claude-code"),
    ("CLAUDE_CODE", "claude-code"),
    ("CODEX_THREAD_ID", "codex"),
    ("CODEX_SANDBOX", "codex"),
    ("CODEX_CI", "codex"),
    ("CURSOR_AGENT", "cursor"),
    ("GEMINI_CLI", "gemini"),
    ("COPILOT_ALLOW_ALL", "github-copilot"),
    ("OPENCODE", "opencode"),
    ("OPENCODE_CLIENT", "opencode"),
    ("CLINE_ACTIVE", "cline"),
    ("ROO_ACTIVE", "roo-code"),
    ("PI_CODING_AGENT", "pi"),
    ("GOOSE_TERMINAL", "goose"),
    ("QWEN_CODE", "qwen-code"),
    ("CRUSH", "crush"),
    ("AMP_CURRENT_THREAD_ID", "amp"),
    ("KIRO_SESSION_ID", "kiro"),
    ("JUNIE_SHIM_PATH", "junie"),
    ("ANTIGRAVITY_AGENT", "antigravity"),
    ("AUGMENT_AGENT", "augment"),
];

const FALSE_VALUES: &[&str] = &["0", "false", "no", "off"];
const TRUE_VALUES: &[&str] = &["1", "true", "yes", "on"];
const MAX_VALUE_LEN: usize = 64;

/// The name of the AI coding agent running this process, or `None` when there is none.
///
/// `AI_AGENT` wins and names the agent; a false value such as `0` opts out. Otherwise a known
/// agent's own variable, then a generic `AGENT`, mark the process (`AGENT=1` gives `unknown`).
pub fn detect_ai_agent() -> Option<String> {
    detect_ai_agent_in(|name| std::env::var_os(name))
}

/// The [`AI_AGENT_HEADER`] value: the detected agent reduced to a single bounded token, or
/// [`NO_AI_AGENT`].
pub fn ai_agent_header_value() -> HeaderValue {
    header_value(detect_ai_agent())
}

fn header_value(agent: Option<String>) -> HeaderValue {
    let token: String = agent
        .unwrap_or_default()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "._+-".contains(c) {
                c
            } else {
                '_'
            }
        })
        .take(MAX_VALUE_LEN)
        .collect();
    let token = if token.is_empty() {
        NO_AI_AGENT.to_string()
    } else {
        token
    };
    HeaderValue::from_str(&token).expect("token is a valid header value")
}

fn detect_ai_agent_in(lookup: impl Fn(&str) -> Option<OsString>) -> Option<String> {
    let value = |name: &str| {
        lookup(name)
            .map(|value| value.to_string_lossy().trim().to_string())
            .unwrap_or_default()
    };
    let ai_agent = value("AI_AGENT");
    if FALSE_VALUES.contains(&ai_agent.to_lowercase().as_str()) {
        return None;
    }
    if !ai_agent.is_empty() {
        return Some(ai_agent);
    }
    if let Some((_, agent)) = AGENT_ENV_VARS
        .iter()
        .find(|(var, _)| lookup(var).is_some_and(|value| !value.is_empty()))
    {
        return Some((*agent).to_string());
    }
    let agent = value("AGENT");
    let lowered = agent.to_lowercase();
    if agent.is_empty() || FALSE_VALUES.contains(&lowered.as_str()) {
        None
    } else if TRUE_VALUES.contains(&lowered.as_str()) {
        Some("unknown".to_string())
    } else {
        Some(agent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Environment variables as name and value pairs.
    type Vars<'a> = &'a [(&'a str, &'a str)];

    fn detected(vars: Vars) -> Option<String> {
        let env: HashMap<&str, OsString> = vars
            .iter()
            .map(|(name, value)| (*name, OsString::from(value)))
            .collect();
        detect_ai_agent_in(|name| env.get(name).cloned())
    }

    /// The Python client's cases.
    #[test]
    fn matches_python_client() {
        let cases: &[(Vars, Option<&str>)] = &[
            (&[], None),
            (
                &[("AI_AGENT", "claude-code_2-1-326_agent")],
                Some("claude-code_2-1-326_agent"),
            ),
            (
                &[("AI_AGENT", "devin@1"), ("CLAUDECODE", "1")],
                Some("devin@1"),
            ),
            (&[("AI_AGENT", "0"), ("CLAUDECODE", "1")], None),
            (
                &[("AI_AGENT", "  "), ("CLAUDECODE", "1")],
                Some("claude-code"),
            ),
            (&[("CODEX_THREAD_ID", "019a")], Some("codex")),
            (
                &[("KILO_PID", "4242"), ("OPENCODE", "1"), ("AGENT", "1")],
                Some("kilo-code"),
            ),
            (&[("AGENT", "goose")], Some("goose")),
            (&[("AGENT", "1")], Some("unknown")),
            (&[("AGENT", "false")], None),
            (&[("CLAUDECODE", "")], None),
            (&[("CURSOR_AGENT", "1")], Some("cursor")),
            (
                &[("CURSOR_TRACE_ID", "abc"), ("TERM_PROGRAM", "vscode")],
                None,
            ),
        ];
        for (vars, expected) in cases {
            assert_eq!(detected(vars).as_deref(), *expected, "{vars:?}");
        }
    }

    #[test]
    fn each_agent_variable_names_its_agent() {
        for (var, agent) in AGENT_ENV_VARS {
            assert_eq!(detected(&[(var, "1")]).as_deref(), Some(*agent), "{var}");
        }
    }

    /// Environments recorded on a developer machine with current releases of each tool.
    #[test]
    fn recorded_environments() {
        let cases: &[(&str, Vars, Option<&str>)] = &[
            (
                "Claude Code agent",
                &[
                    ("AI_AGENT", "claude-code_2-1-326_agent"),
                    ("CLAUDECODE", "1"),
                    ("CLAUDE_CODE_ENTRYPOINT", "x"),
                ],
                Some("claude-code_2-1-326_agent"),
            ),
            (
                "Cursor agent",
                &[
                    ("CURSOR_AGENT", "1"),
                    ("CURSOR_CONVERSATION_ID", "x"),
                    ("VSCODE_PID", "1"),
                ],
                Some("cursor"),
            ),
            (
                "Codex agent",
                &[
                    ("CODEX_CI", "1"),
                    ("CODEX_THREAD_ID", "x"),
                    ("CODEX_SANDBOX_NETWORK_DISABLED", "1"),
                ],
                Some("codex"),
            ),
            (
                "Cursor terminal a person types in",
                &[("TERM_PROGRAM", "vscode"), ("VSCODE_INJECTION", "1")],
                None,
            ),
            (
                "Claude desktop terminal a person types in",
                &[("TERM_PROGRAM", "claude-desktop")],
                None,
            ),
        ];
        for (label, vars, expected) in cases {
            assert_eq!(detected(vars).as_deref(), *expected, "{label}");
        }
    }

    #[test]
    fn opt_out_and_flags_ignore_case_and_whitespace() {
        assert_eq!(
            detected(&[("AI_AGENT", " OFF "), ("CLAUDECODE", "1")]),
            None
        );
        assert_eq!(detected(&[("AI_AGENT", "No")]), None);
        assert_eq!(detected(&[("AGENT", " TRUE ")]).as_deref(), Some("unknown"));
        assert_eq!(detected(&[("AGENT", "Off")]), None);
        assert_eq!(detected(&[("AGENT", "  ")]), None);
    }

    #[test]
    fn agent_variable_beats_a_false_generic_agent() {
        assert_eq!(
            detected(&[("AGENT", "0"), ("CODEX_CI", "1")]).as_deref(),
            Some("codex")
        );
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_value_still_names_an_agent() {
        use std::os::unix::ffi::OsStringExt;
        let value = OsString::from_vec(vec![b'a', 0xff, b'b']);
        let agent = detect_ai_agent_in(|name| (name == "AI_AGENT").then(|| value.clone()));
        assert_eq!(header_value(agent), "a_b");
    }

    /// The Python client's header value cases.
    #[test]
    fn header_value_is_a_single_bounded_token() {
        let long = format!("my agent (v2)/x{}", "a".repeat(100));
        let cases: &[(Option<&str>, &str)] = &[
            (None, "none"),
            (Some(""), "none"),
            (Some("claude-code"), "claude-code"),
            (Some("devin@1"), "devin_1"),
        ];
        for (agent, expected) in cases {
            assert_eq!(header_value(agent.map(str::to_string)), *expected);
        }
        let expected: String = format!("my_agent__v2__x{}", "a".repeat(100))
            .chars()
            .take(64)
            .collect();
        assert_eq!(header_value(Some(long)), expected.as_str());
    }
}

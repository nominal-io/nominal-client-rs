//! Reports in the `User-Agent` header whether an AI coding agent is running the process.
//!
//! Requests carry an `ai-agent/1` product token when an agent marks the process and `ai-agent/0`
//! when none does, so Nominal can tell agent-driven use apart from other use and from releases
//! that never checked. The version is a number because the Conjure HTTP client accepts only
//! numeric versions.

use std::ffi::OsString;

/// Product name of the AI-agent token in the `User-Agent` header.
pub const AI_AGENT_PRODUCT: &str = "ai-agent";

/// Variables AI coding agents set in the commands they run. Drawn from @vercel/detect-agent,
/// laravel/agent-detector, and each agent's own source; Cursor's and Codex's were checked against
/// current releases.
const AGENT_ENV_VARS: &[&str] = &[
    "KILO_PID",
    "CLAUDECODE",
    "CLAUDE_CODE",
    "CODEX_THREAD_ID",
    "CODEX_SANDBOX",
    "CODEX_CI",
    "CURSOR_AGENT",
    "GEMINI_CLI",
    "COPILOT_ALLOW_ALL",
    "OPENCODE",
    "OPENCODE_CLIENT",
    "CLINE_ACTIVE",
    "ROO_ACTIVE",
    "PI_CODING_AGENT",
    "GOOSE_TERMINAL",
    "QWEN_CODE",
    "CRUSH",
    "AMP_CURRENT_THREAD_ID",
    "KIRO_SESSION_ID",
    "JUNIE_SHIM_PATH",
    "ANTIGRAVITY_AGENT",
    "AUGMENT_AGENT",
];

const FALSE_VALUES: &[&str] = &["0", "false", "no", "off"];

/// Whether an AI coding agent is running this process.
///
/// `AI_AGENT` wins: any value marks the process, and a false value such as `0` opts out.
/// Otherwise any known agent's own variable, or a generic `AGENT` that isn't false, marks it.
pub fn ai_agent_detected() -> bool {
    ai_agent_detected_in(|name| std::env::var_os(name))
}

/// The AI-agent token's version: `1` when [`ai_agent_detected`], else `0`.
pub fn ai_agent_version() -> &'static str {
    if ai_agent_detected() { "1" } else { "0" }
}

/// Appends the AI-agent token to `product`, such as `nominal-cli/1.2.3`, giving a `User-Agent`
/// value such as `nominal-cli/1.2.3 ai-agent/1`.
pub fn with_ai_agent(product: &str) -> String {
    format!("{product} {AI_AGENT_PRODUCT}/{}", ai_agent_version())
}

fn ai_agent_detected_in(lookup: impl Fn(&str) -> Option<OsString>) -> bool {
    let value = |name: &str| {
        lookup(name)
            .map(|value| value.to_string_lossy().trim().to_lowercase())
            .unwrap_or_default()
    };
    let ai_agent = value("AI_AGENT");
    if !ai_agent.is_empty() {
        return !FALSE_VALUES.contains(&ai_agent.as_str());
    }
    if AGENT_ENV_VARS
        .iter()
        .any(|var| lookup(var).is_some_and(|value| !value.is_empty()))
    {
        return true;
    }
    let agent = value("AGENT");
    !agent.is_empty() && !FALSE_VALUES.contains(&agent.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn detected(vars: &[(&str, &str)]) -> bool {
        let env: HashMap<&str, OsString> = vars
            .iter()
            .map(|(name, value)| (*name, OsString::from(value)))
            .collect();
        ai_agent_detected_in(|name| env.get(name).cloned())
    }

    /// Environment variables as name and value pairs.
    type Vars<'a> = &'a [(&'a str, &'a str)];

    /// The Python client's cases: it reports `none` exactly where this reports no agent.
    #[test]
    fn matches_python_client() {
        let cases: &[(Vars, bool)] = &[
            (&[], false),
            (&[("AI_AGENT", "claude-code_2-1-326_agent")], true),
            (&[("AI_AGENT", "devin@1"), ("CLAUDECODE", "1")], true),
            (&[("AI_AGENT", "0"), ("CLAUDECODE", "1")], false),
            (&[("AI_AGENT", "  "), ("CLAUDECODE", "1")], true),
            (&[("CODEX_THREAD_ID", "019a")], true),
            (
                &[("KILO_PID", "4242"), ("OPENCODE", "1"), ("AGENT", "1")],
                true,
            ),
            (&[("AGENT", "goose")], true),
            (&[("AGENT", "1")], true),
            (&[("AGENT", "false")], false),
            (&[("CLAUDECODE", "")], false),
            (&[("CURSOR_AGENT", "1")], true),
            (
                &[("CURSOR_TRACE_ID", "abc"), ("TERM_PROGRAM", "vscode")],
                false,
            ),
        ];
        for (vars, expected) in cases {
            assert_eq!(detected(vars), *expected, "{vars:?}");
        }
    }

    #[test]
    fn each_agent_variable_marks_the_process() {
        for var in AGENT_ENV_VARS {
            assert!(detected(&[(var, "1")]), "{var}");
        }
    }

    /// Environments recorded on a developer machine with current releases of each tool.
    #[test]
    fn recorded_environments() {
        let cases: &[(&str, Vars, bool)] = &[
            (
                "Claude Code agent",
                &[
                    ("AI_AGENT", "claude-code_2-1-326_agent"),
                    ("CLAUDECODE", "1"),
                    ("CLAUDE_CODE_ENTRYPOINT", "x"),
                ],
                true,
            ),
            (
                "Cursor agent",
                &[
                    ("CURSOR_AGENT", "1"),
                    ("CURSOR_CONVERSATION_ID", "x"),
                    ("VSCODE_PID", "1"),
                ],
                true,
            ),
            (
                "Codex agent",
                &[
                    ("CODEX_CI", "1"),
                    ("CODEX_THREAD_ID", "x"),
                    ("CODEX_SANDBOX_NETWORK_DISABLED", "1"),
                ],
                true,
            ),
            (
                "Cursor terminal a person types in",
                &[("TERM_PROGRAM", "vscode"), ("VSCODE_INJECTION", "1")],
                false,
            ),
            (
                "Claude desktop terminal a person types in",
                &[("TERM_PROGRAM", "claude-desktop")],
                false,
            ),
        ];
        for (label, vars, expected) in cases {
            assert_eq!(detected(vars), *expected, "{label}");
        }
    }

    #[test]
    fn opt_out_and_flags_ignore_case_and_whitespace() {
        assert!(!detected(&[("AI_AGENT", " OFF "), ("CLAUDECODE", "1")]));
        assert!(!detected(&[("AI_AGENT", "No")]));
        assert!(detected(&[("AGENT", " TRUE ")]));
        assert!(!detected(&[("AGENT", "Off")]));
        assert!(!detected(&[("AGENT", "  ")]));
    }

    #[test]
    fn agent_variable_beats_a_false_generic_agent() {
        assert!(detected(&[("AGENT", "0"), ("CODEX_CI", "1")]));
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_value_still_marks_the_process() {
        use std::os::unix::ffi::OsStringExt;
        let value = OsString::from_vec(vec![b'a', 0xff, b'b']);
        assert!(ai_agent_detected_in(
            |name| (name == "AI_AGENT").then(|| value.clone())
        ));
    }

    #[test]
    fn token_is_a_valid_conjure_agent_and_header() {
        for version in ["0", "1"] {
            let agent = conjure_runtime::Agent::new(AI_AGENT_PRODUCT, version);
            assert_eq!(agent.version(), version, "Conjure keeps the version");
            http::HeaderValue::from_str(&format!(
                "nominal-rust/1.2.3 {AI_AGENT_PRODUCT}/{version}"
            ))
            .unwrap();
        }
    }

    #[test]
    fn with_ai_agent_appends_the_token() {
        assert_eq!(
            with_ai_agent("nominal-cli/1.2.3"),
            format!("nominal-cli/1.2.3 ai-agent/{}", ai_agent_version())
        );
    }
}

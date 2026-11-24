//! Reports which AI coding agent, if any, is running the process in the `User-Agent` header.
//!
//! Requests carry an `ai-agent/<name>` product token, or `ai-agent/none` when no agent marks the
//! process, so Nominal can tell agent-driven use apart from other use and from releases that never
//! checked. The Nominal Python client sends the same token.

use std::ffi::OsString;

/// The value [`detect_ai_agent`] returns when no AI agent marks the process.
pub const NO_AI_AGENT: &str = "none";

/// Product name of the AI-agent token in the `User-Agent` header.
const AI_AGENT_PRODUCT: &str = "ai-agent";

/// Longest agent name sent; longer names are cut.
const MAX_AI_AGENT_LEN: usize = 64;

/// Variables an AI coding agent sets in the shells it runs commands in, with the agent's name,
/// checked in this order. Drawn from @vercel/detect-agent, laravel/agent-detector, and each
/// agent's own source.
const AGENT_ENV_VARS: &[(&str, &str)] = &[
    // Kilo also sets OPENCODE, so it comes first.
    ("KILO_PID", "kilo-code"),
    ("CLAUDECODE", "claude-code"),
    ("CLAUDE_CODE", "claude-code"),
    ("CODEX_THREAD_ID", "codex"),
    ("CODEX_SANDBOX", "codex"),
    ("CODEX_CI", "codex"),
    ("CURSOR_AGENT", "cursor"),
    // Also set in Cursor terminals a person types in; counted as agent use on purpose.
    ("CURSOR_TRACE_ID", "cursor"),
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

/// Returns the AI coding agent running this process as a user-agent token value, or
/// [`NO_AI_AGENT`] when there is none.
///
/// `AI_AGENT` wins and names the agent; a false value such as `0` opts out. Otherwise a known
/// agent's own variable, then the older generic `AGENT`, mark the process as agent-run. Nominal
/// maps the name to the agents it knows, so an approximate name is fine. The result contains only
/// ASCII letters, digits and `.`, `_`, `+`, `-`, and is at most 64 characters long.
pub fn detect_ai_agent() -> String {
    detect_ai_agent_in(|name| std::env::var_os(name))
}

/// Appends the AI-agent token to `product`, such as `nominal-cli/1.2.3`, giving a `User-Agent`
/// value such as `nominal-cli/1.2.3 ai-agent/claude-code`.
pub fn with_ai_agent(product: &str) -> String {
    format!("{product} {AI_AGENT_PRODUCT}/{}", detect_ai_agent())
}

fn detect_ai_agent_in(lookup: impl Fn(&str) -> Option<OsString>) -> String {
    let value = |name: &str| {
        lookup(name)
            .map(|value| value.to_string_lossy().trim().to_string())
            .unwrap_or_default()
    };
    let ai_agent = value("AI_AGENT");
    if is_one_of(&ai_agent, FALSE_VALUES) {
        return NO_AI_AGENT.to_string();
    }
    let mut name = if ai_agent.is_empty() {
        AGENT_ENV_VARS
            .iter()
            .find(|(var, _)| lookup(var).is_some_and(|value| !value.is_empty()))
            .map(|(_, agent)| agent.to_string())
            .unwrap_or_default()
    } else {
        ai_agent
    };
    if name.is_empty() {
        let agent = value("AGENT");
        if !agent.is_empty() && !is_one_of(&agent, FALSE_VALUES) {
            name = if is_one_of(&agent, TRUE_VALUES) {
                "unknown".to_string()
            } else {
                agent
            };
        }
    }
    let token: String = name
        .chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '.' | '_' | '+' | '-' => c,
            _ => '_',
        })
        .take(MAX_AI_AGENT_LEN)
        .collect();
    if token.is_empty() {
        NO_AI_AGENT.to_string()
    } else {
        token
    }
}

fn is_one_of(value: &str, values: &[&str]) -> bool {
    let value = value.to_lowercase();
    values.contains(&value.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::HeaderValue;
    use std::collections::HashMap;

    fn detect(vars: &[(&str, &str)]) -> String {
        let env: HashMap<&str, OsString> = vars
            .iter()
            .map(|(name, value)| (*name, OsString::from(value)))
            .collect();
        detect_ai_agent_in(|name| env.get(name).cloned())
    }

    /// The Python client's cases, with the same results.
    #[test]
    fn matches_python_client() {
        let cases: &[(&[(&str, &str)], &str)] = &[
            (&[], "none"),
            (
                &[("AI_AGENT", "claude-code_2-1-326_agent")],
                "claude-code_2-1-326_agent",
            ),
            (&[("AI_AGENT", "devin@1"), ("CLAUDECODE", "1")], "devin_1"),
            (&[("AI_AGENT", "0"), ("CLAUDECODE", "1")], "none"),
            (&[("AI_AGENT", "  "), ("CLAUDECODE", "1")], "claude-code"),
            (&[("CODEX_THREAD_ID", "019a")], "codex"),
            (
                &[("KILO_PID", "4242"), ("OPENCODE", "1"), ("AGENT", "1")],
                "kilo-code",
            ),
            (&[("AGENT", "goose")], "goose"),
            (&[("AGENT", "1")], "unknown"),
            (&[("AGENT", "false")], "none"),
            (&[("CLAUDECODE", "")], "none"),
        ];
        for (vars, expected) in cases {
            assert_eq!(detect(vars), *expected, "{vars:?}");
        }
    }

    #[test]
    fn each_agent_variable_names_its_agent() {
        for (var, agent) in AGENT_ENV_VARS {
            assert_eq!(detect(&[(var, "1")]), *agent, "{var}");
        }
    }

    #[test]
    fn earlier_agent_variables_win() {
        for (i, (later_var, _)) in AGENT_ENV_VARS.iter().enumerate().skip(1) {
            let (earlier_var, earlier_agent) = AGENT_ENV_VARS[i - 1];
            assert_eq!(
                detect(&[(later_var, "1"), (earlier_var, "1")]),
                earlier_agent,
                "{earlier_var} before {later_var}"
            );
        }
    }

    #[test]
    fn opt_out_and_flags_ignore_case_and_whitespace() {
        assert_eq!(
            detect(&[("AI_AGENT", " OFF "), ("CLAUDECODE", "1")]),
            "none"
        );
        assert_eq!(detect(&[("AI_AGENT", "No")]), "none");
        assert_eq!(detect(&[("AGENT", " TRUE ")]), "unknown");
        assert_eq!(detect(&[("AGENT", "Yes")]), "unknown");
        assert_eq!(detect(&[("AGENT", "Off")]), "none");
        assert_eq!(detect(&[("AGENT", "  ")]), "none");
    }

    #[test]
    fn agent_variable_beats_generic_agent() {
        assert_eq!(detect(&[("AGENT", "goose"), ("CODEX_CI", "1")]), "codex");
    }

    #[test]
    fn ai_agent_value_is_trimmed_and_kept() {
        assert_eq!(detect(&[("AI_AGENT", "  my-agent  ")]), "my-agent");
        assert_eq!(detect(&[("AI_AGENT", "a.b+c_d-e")]), "a.b+c_d-e");
    }

    #[test]
    fn value_is_reduced_to_one_bounded_token() {
        let long = format!("my agent (v2)/x{}", "a".repeat(100));
        let expected: String = format!("my_agent__v2__x{}", "a".repeat(100))
            .chars()
            .take(64)
            .collect();
        assert_eq!(detect(&[("AI_AGENT", &long)]), expected);
        assert_eq!(detect(&[("AI_AGENT", "é/ü")]), "___");
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_value_is_replaced() {
        use std::os::unix::ffi::OsStringExt;
        let env = OsString::from_vec(vec![b'a', 0xff, b'b']);
        assert_eq!(
            detect_ai_agent_in(|name| (name == "AI_AGENT").then(|| env.clone())),
            "a_b"
        );
    }

    /// Every character an agent name could contain leaves a single valid `ai-agent/<name>` token,
    /// so the header stays valid and no value can add or split tokens.
    #[test]
    fn any_value_gives_a_valid_single_token() {
        let mut values: Vec<String> = (0u32..=0x2FF)
            .chain([0x2028, 0x3000, 0xFEFF, 0xFFFD, 0x1F600])
            .filter_map(char::from_u32)
            .map(|c| format!("a{c}b"))
            .collect();
        values.extend(
            [
                "devin@1",
                "a b",
                "a\tb",
                "a\r\nX-Injected: 1",
                "a/b",
                "a;b",
                "a,b",
                "a(b)",
                "a\"b",
                "a\\b",
                "a=b",
                "a:b",
                "a<b>",
                "a{b}",
                "a[b]",
                "a?b",
                "\u{0}",
            ]
            .map(String::from),
        );
        for value in values {
            let token = detect(&[("AI_AGENT", &value)]);
            assert!(
                !token.is_empty() && token.len() <= MAX_AI_AGENT_LEN,
                "{value:?} -> {token:?}"
            );
            // A subset of RFC 9110 tchar.
            assert!(
                token
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || ".+_-".contains(c)),
                "{value:?} -> {token:?}"
            );
            let header = format!("nominal-rust/1.2.3 {AI_AGENT_PRODUCT}/{token}");
            HeaderValue::from_str(&header)
                .unwrap_or_else(|e| panic!("{value:?} -> {header:?}: {e}"));
            assert_eq!(
                header.split_whitespace().collect::<Vec<_>>(),
                ["nominal-rust/1.2.3", &format!("ai-agent/{token}")],
                "{value:?}"
            );
        }
    }

    #[test]
    fn with_ai_agent_appends_the_detected_token() {
        assert_eq!(
            with_ai_agent("nominal-cli/1.2.3"),
            format!("nominal-cli/1.2.3 ai-agent/{}", detect_ai_agent())
        );
    }
}

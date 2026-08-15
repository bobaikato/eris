//! Shared OpenAI `/chat/completions` wire helpers for HTTP backends
//! ([`crate::engine::llama_cpp::LlamaCppClient`] and [`crate::engine::openrouter::OpenRouterClient`]).
//! Hosted models are just as strict about role ordering as local chat templates, so both
//! backends normalize through one copy — the shapes cannot drift apart.

use serde::Serialize;

/// One wire message for the OpenAI chat API.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct ChatMsg {
    pub role: String,
    pub content: String,
}

/// Normalize messages for chat templates that require all system content at
/// the beginning (e.g. Qwen).  Merge leading consecutive system messages into
/// one; re-role any later system messages as "user" so the wire payload never
/// violates the "system-only-at-start" invariant.
pub(crate) fn normalize_system_messages(messages: Vec<ChatMsg>) -> Vec<ChatMsg> {
    if messages.is_empty() {
        return messages;
    }

    let leading_system_count = messages
        .iter()
        .take_while(|m| m.role == "system")
        .count();

    let mut out = Vec::with_capacity(messages.len());

    if leading_system_count > 1 {
        let merged: String = messages[..leading_system_count]
            .iter()
            .map(|m| m.content.as_str())
            .collect::<Vec<_>>()
            .join("\n\n---\n\n");
        out.push(ChatMsg {
            role: "system".to_string(),
            content: merged,
        });
    } else if leading_system_count == 1 {
        out.push(ChatMsg {
            role: messages[0].role.clone(),
            content: messages[0].content.clone(),
        });
    }

    let mut had_stray = false;
    for m in messages.into_iter().skip(leading_system_count) {
        if m.role == "system" {
            had_stray = true;
            out.push(ChatMsg {
                role: "user".to_string(),
                content: format!("[System] {}", m.content),
            });
        } else {
            out.push(m);
        }
    }

    if had_stray {
        tracing::warn!(
            "openai_wire: stray system messages after non-system rows re-roled as user for strict chat template"
        );
    }

    out
}

/// OpenAI-compatible servers reject requests where two or more `assistant` messages appear
/// at the end of `messages` (`invalid_request_error`). The orchestrator stack can legitimately
/// end with several assistant rows (e.g. failed protocol JSON kept for recovery). Merge trailing
/// assistant messages into one wire message so the API accepts the payload.
pub(crate) fn merge_trailing_assistant_messages(mut messages: Vec<ChatMsg>) -> Vec<ChatMsg> {
    if messages.len() < 2 {
        return messages;
    }
    let n = messages.len();
    let mut tail_asst = 0usize;
    for i in (0..n).rev() {
        if messages[i].role == "assistant" {
            tail_asst += 1;
        } else {
            break;
        }
    }
    if tail_asst < 2 {
        return messages;
    }
    let start = n - tail_asst;
    tracing::debug!(
        tail_asst,
        "openai_wire: merging trailing assistant messages for OpenAI wire format"
    );
    const SEP: &str = "\n\n---[FCP prior assistant message]---\n\n";
    let merged_content: String = messages[start..]
        .iter()
        .map(|m| m.content.as_str())
        .collect::<Vec<_>>()
        .join(SEP);
    messages.truncate(start);
    messages.push(ChatMsg {
        role: "assistant".into(),
        content: merged_content,
    });
    messages
}

/// Convert the engine-neutral stack into wire messages, applying both normalizations.
pub(crate) fn to_wire_messages(stack: &[crate::engine::Message]) -> Vec<ChatMsg> {
    let raw: Vec<ChatMsg> = stack
        .iter()
        .map(|m| ChatMsg {
            role: m.role.clone(),
            content: m.content.clone(),
        })
        .collect();
    merge_trailing_assistant_messages(normalize_system_messages(raw))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sys(s: &str) -> ChatMsg {
        ChatMsg {
            role: "system".into(),
            content: s.into(),
        }
    }
    fn user(s: &str) -> ChatMsg {
        ChatMsg {
            role: "user".into(),
            content: s.into(),
        }
    }
    fn asst(s: &str) -> ChatMsg {
        ChatMsg {
            role: "assistant".into(),
            content: s.into(),
        }
    }

    mod normalize_system_messages_tests {
        use super::*;

        #[test]
        fn empty_stack_unchanged() {
            let out = normalize_system_messages(vec![]);
            assert!(out.is_empty());
        }

        #[test]
        fn single_system_at_front_unchanged() {
            let out = normalize_system_messages(vec![sys("prompt"), user("hi")]);
            assert_eq!(out.len(), 2);
            assert_eq!(out[0].role, "system");
            assert_eq!(out[0].content, "prompt");
            assert_eq!(out[1].role, "user");
        }

        #[test]
        fn multiple_leading_systems_merged() {
            let out = normalize_system_messages(vec![
                sys("main"),
                sys("rolling summary"),
                user("hi"),
            ]);
            assert_eq!(out.len(), 2);
            assert_eq!(out[0].role, "system");
            assert!(out[0].content.contains("main"));
            assert!(out[0].content.contains("rolling summary"));
            assert_eq!(out[1].role, "user");
        }

        #[test]
        fn stray_system_after_user_reroled() {
            let out = normalize_system_messages(vec![
                sys("prompt"),
                user("hello"),
                asst("hi back"),
                sys("Tool 'x:y' succeeded: data"),
            ]);
            assert_eq!(out.len(), 4);
            assert_eq!(out[0].role, "system");
            assert_eq!(out[3].role, "user");
            assert!(out[3].content.starts_with("[System]"));
            assert!(out[3].content.contains("Tool 'x:y' succeeded: data"));
        }

        #[test]
        fn realistic_tool_turn_stack() {
            let out = normalize_system_messages(vec![
                sys("prompt"),
                user("weather?"),
                asst("{tool_calls: ...}"),
                sys("Tool 'weather:get' succeeded: 25°C"),
                sys("POST_TOOL_GUIDANCE"),
                sys("JIT guidance"),
            ]);
            assert_eq!(out[0].role, "system");
            assert_eq!(out[0].content, "prompt");
            for m in &out[1..] {
                assert_ne!(m.role, "system", "no system messages after index 0");
            }
            assert_eq!(out[3].role, "user");
            assert!(out[3].content.contains("weather:get"));
        }

        #[test]
        fn no_system_messages_at_all() {
            let out = normalize_system_messages(vec![user("hi"), asst("hello")]);
            assert_eq!(out.len(), 2);
            assert_eq!(out[0].role, "user");
            assert_eq!(out[1].role, "assistant");
        }
    }

    mod merge_trailing_assistant_messages_tests {
        use super::*;

        #[test]
        fn empty_and_single_unchanged() {
            assert!(merge_trailing_assistant_messages(vec![]).is_empty());
            let one = vec![asst("only")];
            let out = merge_trailing_assistant_messages(one);
            assert_eq!(out.len(), 1);
            assert_eq!(out[0].content, "only");
        }

        #[test]
        fn two_trailing_assistants_merged() {
            let out = merge_trailing_assistant_messages(vec![user("u"), asst("a1"), asst("a2")]);
            assert_eq!(out.len(), 2);
            assert_eq!(out[0].role, "user");
            assert_eq!(out[1].role, "assistant");
            assert!(out[1].content.contains("a1"));
            assert!(out[1].content.contains("a2"));
            assert!(out[1].content.contains("[FCP prior assistant message]"));
        }

        #[test]
        fn internal_assistant_pair_not_merged() {
            let out = merge_trailing_assistant_messages(vec![
                user("u1"),
                asst("mid1"),
                asst("mid2"),
                user("u2"),
                asst("last"),
            ]);
            assert_eq!(out.len(), 5);
            assert_eq!(out[3].role, "user");
            assert_eq!(out[4].role, "assistant");
            assert_eq!(out[4].content, "last");
        }

        #[test]
        fn three_trailing_assistants_one_block() {
            let out = merge_trailing_assistant_messages(vec![
                user("u"),
                asst("x"),
                asst("y"),
                asst("z"),
            ]);
            assert_eq!(out.len(), 2);
            assert_eq!(out[1].role, "assistant");
            assert!(out[1].content.contains('x'));
            assert!(out[1].content.contains('y'));
            assert!(out[1].content.contains('z'));
        }
    }
}

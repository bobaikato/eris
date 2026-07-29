//! Lightweight signal extraction for pre-LLM routing (Phase 2).
//!
//! No new ML — boolean / lexical cues plus embed hits and dialog context.

/// Snapshot of inputs the policy rules may read.
#[derive(Debug, Clone)]
pub struct RoutingSignals {
    pub user_text: String,
    /// Router hits already sorted descending (embed + lexical forced).
    pub embed_hits: Vec<(String, f32)>,
    /// Session-scoped recent successes (newest last).
    pub recent_successful_tools: Vec<String>,
    pub has_url: bool,
    pub agenda_continuation: bool,
    pub doc_ingest_cues: bool,
    pub recent_had_agenda: bool,
}

impl RoutingSignals {
    #[must_use]
    pub fn from_turn(
        user_text: &str,
        embed_hits: Vec<(String, f32)>,
        recent_successful_tools: &[String],
    ) -> Self {
        let lower = user_text.to_ascii_lowercase();
        let has_url = lower.contains("http://")
            || lower.contains("https://")
            || lower.contains("www.");
        let recent_had_agenda = recent_successful_tools
            .iter()
            .any(|n| n.starts_with("agenda:"));
        Self {
            user_text: user_text.to_string(),
            embed_hits,
            recent_successful_tools: recent_successful_tools.to_vec(),
            has_url,
            agenda_continuation: has_agenda_continuation_intent(user_text),
            doc_ingest_cues: has_doc_ingest_cues(user_text),
            recent_had_agenda,
        }
    }
}

/// User wants to close/remove/finish something after seeing agenda.
#[must_use]
pub fn has_agenda_continuation_intent(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    const CUES: &[&str] = &[
        "remove",
        "delete",
        "done",
        "complete",
        "finished",
        "finish",
        "clear it",
        "clear that",
        "mark as done",
        "check off",
        "crossed off",
        "take it off",
        "off the agenda",
        "from the agenda",
        "from my agenda",
    ];
    CUES.iter().any(|c| lower.contains(c))
}

/// Explicit document-ingest intent — do not steal the turn for agenda pairing.
#[must_use]
pub fn has_doc_ingest_cues(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("ingest")
        || lower.contains("upload")
        || lower.contains(".pdf")
        || lower.contains("99_user_uploaded")
        || lower.contains("document rag")
        || (lower.contains("document")
            && (lower.contains("add") || lower.contains("index") || lower.contains("import")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agenda_cues_and_doc_cues() {
        assert!(has_agenda_continuation_intent("done"));
        assert!(has_agenda_continuation_intent("remove that please"));
        assert!(!has_agenda_continuation_intent("what's the weather"));
        assert!(has_doc_ingest_cues("ingest report.pdf please"));
        assert!(!has_doc_ingest_cues("remove it"));
    }
}

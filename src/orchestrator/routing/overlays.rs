//! Slim-offer overlays shared by prompt assembly and GBNF subset selection.
//!
//! Single source of truth for: offer cap, Moltbook latch, `web:find` pairing,
//! and `doc:read` → `vault:write`.

use crate::orchestrator::state::AgentState;
use crate::tools::Gatekeeper;

/// Build the final offered-tool list for slim phrase map + subset grammar.
#[must_use]
pub fn apply_offer_overlays(
    pre_llm_matched_tools: &[String],
    tool_map_offer_cap: usize,
    moltbook_overlay_latched: bool,
    gatekeeper: &Gatekeeper,
    state: &AgentState,
) -> Vec<String> {
    let mut offered: Vec<String> = if pre_llm_matched_tools.is_empty() {
        vec![]
    } else if tool_map_offer_cap == 0 {
        pre_llm_matched_tools.to_vec()
    } else {
        pre_llm_matched_tools
            .iter()
            .take(tool_map_offer_cap)
            .cloned()
            .collect()
    };

    if moltbook_overlay_latched && !offered.is_empty() {
        for name in gatekeeper.allowed_tool_names_with_prefix(state, "moltbook:") {
            if !offered.contains(&name) {
                offered.push(name);
            }
        }
    }

    let needs_web_find = offered.iter().any(|n| n == "web:fetch" || n == "web:search");
    if needs_web_find {
        let find_allowed = gatekeeper
            .allowed_tool_names_with_prefix(state, "web:")
            .into_iter()
            .any(|n| n == "web:find");
        if find_allowed && !offered.iter().any(|n| n == "web:find") {
            offered.push("web:find".to_string());
        }
    }

    if offered.iter().any(|n| n == "doc:read")
        && !offered.iter().any(|n| n == "vault:write")
        && Gatekeeper::state_allows_tool(state, "vault:write")
    {
        offered.push("vault:write".to_string());
    }

    offered
}

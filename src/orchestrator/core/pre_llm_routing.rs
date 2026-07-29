use crate::engine::LlmEngine;
use crate::orchestrator::routing::{
    apply_routing_policy, RoutingPolicyKnobs,
};
use crate::orchestrator::tool_router::ToolRouter;
use crate::presentation::SYSTEM_ALARM_PREFIX;
use crate::telemetry::routing_codes;
use std::time::Instant;

use super::Orchestrator;

impl<E: LlmEngine> Orchestrator<E> {
    /// Conversational vs tool mode, plus ordered router names for Tier 1 (Top-K).
    pub(super) async fn run_pre_llm_routing(&mut self) -> (bool, Vec<String>) {
        let user_input = self.last_user_content().to_string();
        let turn_seq = self.turn_seq;

        if user_input.starts_with(SYSTEM_ALARM_PREFIX) {
            let alarm_payload = user_input
                .strip_prefix(SYSTEM_ALARM_PREFIX)
                .unwrap_or(user_input.as_str());
            if alarm_payload.to_ascii_lowercase().contains("moltbook") {
                tracing::info!(
                    category = routing_codes::CATEGORY_ROUTING,
                    issue = routing_codes::ISSUE_PRELLM_ALARM_TOOL_ELIGIBLE,
                    outcome = routing_codes::outcome_from_pre_llm_tuple(true, 0),
                    turn_seq,
                    tools_needed = true,
                    router_match_count = 0usize,
                    "system alarm prefix with Moltbook label; semantic tool routing enabled"
                );
            } else {
                self.last_router_ms = 0;
                self.last_top_tool_match = None;
                tracing::info!(
                    category = routing_codes::CATEGORY_ROUTING,
                    issue = routing_codes::ISSUE_PRELLM_CONV_ALARM,
                    outcome = routing_codes::OUTCOME_CONVERSATIONAL,
                    turn_seq,
                    tools_needed = false,
                    router_match_count = 0usize,
                    "system alarm prefix; conversational mode"
                );
                return (false, Vec::new());
            }
        }

        if ToolRouter::short_input_guard_conversational_only(&user_input) {
            self.last_router_ms = 0;
            self.last_top_tool_match = None;
            tracing::info!(
                category = routing_codes::CATEGORY_ROUTING,
                issue = routing_codes::ISSUE_PRELLM_CONV_SHORT_INPUT,
                outcome = routing_codes::OUTCOME_CONVERSATIONAL,
                turn_seq,
                tools_needed = false,
                router_match_count = 0usize,
                "short-input guard; conversational mode"
            );
            return (false, Vec::new());
        }

        let router_started = Instant::now();
        let match_result = {
            let Some(router) = self.tool_router.as_ref() else {
                self.last_router_ms = 0;
                self.last_top_tool_match = None;
                tracing::warn!(
                    category = routing_codes::CATEGORY_ROUTING,
                    issue = routing_codes::ISSUE_PRELLM_ROUTER_UNAVAILABLE,
                    outcome = routing_codes::outcome_from_pre_llm_tuple(true, 0),
                    turn_seq,
                    tools_needed = true,
                    router_match_count = 0usize,
                    "no tool router; roster-only tool mode"
                );
                return (true, Vec::new());
            };
            router.match_tools(&user_input).await
        };

        match match_result {
            Ok(matches) if matches.is_empty() => {
                self.last_router_ms = router_started.elapsed().as_millis() as u64;
                self.last_top_tool_match = None;
                tracing::info!(
                    category = routing_codes::CATEGORY_ROUTING,
                    issue = routing_codes::ISSUE_PRELLM_SEMANTIC_EMPTY,
                    outcome = routing_codes::outcome_from_pre_llm_tuple(true, 0),
                    turn_seq,
                    tools_needed = true,
                    router_match_count = 0usize,
                    "no semantic tool match; tool fallback mode"
                );
                (true, Vec::new())
            }
            Ok(matches) => {
                self.last_router_ms = router_started.elapsed().as_millis() as u64;
                let raw_preview: Vec<String> = matches
                    .iter()
                    .map(|(n, s)| format!("{}({:.3})", n, s))
                    .collect();
                let registered = self.gatekeeper.registered_tool_names();
                let knobs = RoutingPolicyKnobs {
                    single_hit_floor: self.config.tool_single_hit_floor,
                    match_margin: self.config.tool_match_margin,
                };
                let recent = self.recent_successful_tools.clone();
                let policy = apply_routing_policy(
                    &user_input,
                    &matches,
                    &recent,
                    &registered,
                    knobs,
                );
                self.last_top_tool_match = policy
                    .offered
                    .first()
                    .cloned()
                    .or_else(|| matches.first().map(|(n, s)| format!("{n}({s:.3})")));
                let names = policy.offered;
                let router_match_count = names.len();
                let issue = if policy.reason == "EMPTY_AFTER_POLICY" && !matches.is_empty() {
                    routing_codes::ISSUE_PRELLM_POLICY_REWRITE
                } else if policy.reason == "AGENDA_DIALOG_PAIRING"
                    || policy.reason == "MARGIN_CLUSTER_OR_SUBSET"
                {
                    routing_codes::ISSUE_PRELLM_POLICY_REWRITE
                } else {
                    routing_codes::ISSUE_PRELLM_SEMANTIC_HIT
                };
                tracing::info!(
                    category = routing_codes::CATEGORY_ROUTING,
                    issue,
                    outcome = routing_codes::outcome_from_pre_llm_tuple(true, router_match_count),
                    turn_seq,
                    tools_needed = true,
                    router_match_count,
                    policy_reason = policy.reason,
                    raw_matched = ?raw_preview,
                    offered = ?names,
                    "semantic tool match; tool mode (Phase-1 policy applied)"
                );
                (true, names)
            }
            Err(e) => {
                self.last_router_ms = router_started.elapsed().as_millis() as u64;
                self.last_top_tool_match = None;
                tracing::warn!(
                    category = routing_codes::CATEGORY_ROUTING,
                    issue = routing_codes::ISSUE_PRELLM_MATCH_ERROR,
                    outcome = routing_codes::outcome_from_pre_llm_tuple(true, 0),
                    turn_seq,
                    tools_needed = true,
                    router_match_count = 0usize,
                    fcp_error = %e,
                    "pre-LLM match_tools failed; roster-only tool mode"
                );
                (true, Vec::new())
            }
        }
    }

    /// Record a successful tool for dialog-continuation routing (session-scoped, capped).
    pub(super) fn record_successful_tool(&mut self, tool_name: &str) {
        const CAP: usize = 12;
        self.recent_successful_tools.push(tool_name.to_string());
        if self.recent_successful_tools.len() > CAP {
            let drain = self.recent_successful_tools.len() - CAP;
            self.recent_successful_tools.drain(0..drain);
        }
    }
}

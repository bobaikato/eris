//! Phase-1 routing policy: demote weak lock-in, widen near-ties to domain clusters,
//! and pair agenda dialog continuations.

use super::clusters::{
    domains_share_affinity, expand_names_to_domain_clusters, tool_domain, union_clusters_for_tools,
};

/// Lexical / forced hits use this floor so demotion never drops URL/search/news injects.
const FORCED_HIT_FLOOR: f32 = 0.99;

/// Result of applying Phase-1 policy to raw router hits.
#[derive(Debug, Clone, PartialEq)]
pub struct RoutingPolicyResult {
    /// Tool names to offer. Empty + `tools_needed` means full roster (caller convention).
    pub offered: Vec<String>,
    /// Stable reason code for logs (`PRELLM_*`-style short tags).
    pub reason: &'static str,
}

/// Knobs for demotion / margin (from [`crate::config::AppConfig`]).
#[derive(Debug, Clone, Copy)]
pub struct RoutingPolicyKnobs {
    pub single_hit_floor: f32,
    pub match_margin: f32,
}

impl Default for RoutingPolicyKnobs {
    fn default() -> Self {
        Self {
            single_hit_floor: 0.58,
            match_margin: 0.05,
        }
    }
}

/// Apply Phase-1 offer policy.
///
/// `hits` are `(name, score)` already sorted descending (as from [`ToolRouter::match_tools`]).
/// `recent_successful_tools` are session-scoped recent successes (newest last).
/// `registered` is the gatekeeper tool name list used to expand clusters.
#[must_use]
pub fn apply_routing_policy(
    user_text: &str,
    hits: &[(String, f32)],
    recent_successful_tools: &[String],
    registered: &[String],
    knobs: RoutingPolicyKnobs,
) -> RoutingPolicyResult {
    // 1) Agenda dialog pairing — highest priority for the observed doc/agenda miss.
    if let Some(result) =
        try_agenda_dialog_pairing(user_text, hits, recent_successful_tools, registered)
    {
        return result;
    }

    let (forced, embed): (Vec<_>, Vec<_>) = hits
        .iter()
        .cloned()
        .partition(|(_, score)| *score >= FORCED_HIT_FLOOR);

    // 2) Lone weak embed hit → drop (asymmetric: empty is safer than lock-in).
    let embed = demote_lone_weak_embed(embed, knobs.single_hit_floor);

    // 3) Near-tie across *related* domains → cluster union; else keep ranked hits.
    let embed_offer = widen_near_tie_to_clusters(&embed, registered, knobs.match_margin);

    let mut offered = embed_offer;
    for (name, _) in &forced {
        if !offered.contains(name) {
            offered.push(name.clone());
        }
    }

    // Always re-rank by original cosine (no URL hard-pin). Cluster siblings without
    // a direct hit inherit their domain's best seed score.
    offered = rerank_offered_by_cosine(&offered, hits);

    if offered.is_empty() {
        return RoutingPolicyResult {
            offered: Vec::new(),
            reason: "EMPTY_AFTER_POLICY",
        };
    }

    let reason = if forced.is_empty() && embed.len() >= 2 {
        "MARGIN_CLUSTER_OR_SUBSET"
    } else if forced.is_empty() && embed.len() == 1 {
        "SINGLE_STRONG_HIT"
    } else if !forced.is_empty() && embed.is_empty() {
        "LEXICAL_FORCED_ONLY"
    } else {
        "MIXED_EMBED_AND_LEXICAL"
    };

    RoutingPolicyResult { offered, reason }
}

fn demote_lone_weak_embed(
    embed: Vec<(String, f32)>,
    single_hit_floor: f32,
) -> Vec<(String, f32)> {
    if embed.len() == 1 && embed[0].1 < single_hit_floor {
        tracing::info!(
            tool = %embed[0].0,
            score = embed[0].1,
            floor = single_hit_floor,
            event = "routing.policy.single_hit_demoted",
            "Lone weak semantic hit demoted (avoid GBNF lock-in)"
        );
        return Vec::new();
    }
    embed
}

fn widen_near_tie_to_clusters(
    embed: &[(String, f32)],
    registered: &[String],
    match_margin: f32,
) -> Vec<String> {
    if embed.is_empty() {
        return Vec::new();
    }
    if embed.len() == 1 {
        return vec![embed[0].0.clone()];
    }

    let top = embed[0].1;
    let within: Vec<(String, f32)> = embed
        .iter()
        .filter(|(_, s)| top - *s <= match_margin)
        .cloned()
        .collect();

    if within.len() < 2 {
        return embed.iter().map(|(n, _)| n.clone()).collect();
    }

    let Some(top_domain) = tool_domain(&within[0].0) else {
        return embed.iter().map(|(n, _)| n.clone()).collect();
    };

    // Only seeds that share affinity with the top hit participate in cluster union.
    let related_seeds: Vec<(String, f32)> = within
        .iter()
        .filter(|(n, _)| {
            tool_domain(n)
                .map(|d| domains_share_affinity(top_domain, d))
                .unwrap_or(false)
        })
        .cloned()
        .collect();

    let mut related_domains: Vec<&str> = related_seeds
        .iter()
        .filter_map(|(n, _)| tool_domain(n))
        .collect();
    related_domains.sort_unstable();
    related_domains.dedup();

    let mut all_within_domains: Vec<&str> = within
        .iter()
        .filter_map(|(n, _)| tool_domain(n))
        .collect();
    all_within_domains.sort_unstable();
    all_within_domains.dedup();

    // Near-tie within one related domain: keep the ranked hit list.
    // Near-tie across related domains (clock vs agenda): widen to that affinity union.
    // Unrelated multi-domain mush: do not union — keep cosine-ranked embed hits.
    if related_domains.len() >= 2 {
        let seed: Vec<String> = related_seeds.iter().map(|(n, _)| n.clone()).collect();
        let union = union_clusters_for_tools(&seed, registered);
        tracing::info!(
            top_score = top,
            margin = match_margin,
            top_domain,
            related_domains = ?related_domains,
            skipped_unrelated = ?(all_within_domains
                .iter()
                .filter(|d| !related_domains.contains(d))
                .collect::<Vec<_>>()),
            offered_count = union.len(),
            event = "routing.policy.margin_multi_domain_union",
            "Near-tie across related domains; offering affinity cluster union"
        );
        return union;
    }

    if all_within_domains.len() >= 2 {
        tracing::info!(
            top_score = top,
            margin = match_margin,
            top_domain,
            within_domains = ?all_within_domains,
            event = "routing.policy.margin_unrelated_kept_ranked",
            "Near-tie spans unrelated domains; keeping cosine-ranked hits (no cluster dump)"
        );
    }

    embed.iter().map(|(n, _)| n.clone()).collect()
}

/// Order offer names by original router cosine. Unscored cluster siblings inherit
/// the best score among seed hits in the same domain (then name for stability).
#[must_use]
pub fn rerank_offered_by_cosine(offered: &[String], hits: &[(String, f32)]) -> Vec<String> {
    let mut domain_best: Vec<(&str, f32)> = Vec::new();
    for (name, score) in hits {
        if let Some(domain) = tool_domain(name) {
            if let Some((_, best)) = domain_best.iter_mut().find(|(d, _)| *d == domain) {
                if *score > *best {
                    *best = *score;
                }
            } else {
                domain_best.push((domain, *score));
            }
        }
    }

    let score_for = |name: &str| -> f32 {
        if let Some((_, s)) = hits.iter().find(|(n, _)| n == name) {
            return *s;
        }
        if let Some(domain) = tool_domain(name) {
            if let Some((_, best)) = domain_best.iter().find(|(d, _)| *d == domain) {
                // Slightly below the domain's best seed so exact hits sort first.
                return (*best) - 1.0e-4;
            }
        }
        0.0
    };

    let mut ranked = offered.to_vec();
    ranked.sort_by(|a, b| {
        let sa = score_for(a);
        let sb = score_for(b);
        sb.partial_cmp(&sa)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.cmp(b))
    });
    ranked
}

fn try_agenda_dialog_pairing(
    user_text: &str,
    hits: &[(String, f32)],
    recent_successful_tools: &[String],
    registered: &[String],
) -> Option<RoutingPolicyResult> {
    let recent_had_agenda = recent_successful_tools
        .iter()
        .any(|n| n.starts_with("agenda:"));
    if !recent_had_agenda {
        return None;
    }
    if !has_agenda_continuation_intent(user_text) {
        return None;
    }
    if has_doc_ingest_cues(user_text) {
        return None;
    }

    // Suppress bare doc:* embed winners without ingest cues.
    let suppressed_doc = hits
        .iter()
        .any(|(n, s)| n.starts_with("doc:") && *s < FORCED_HIT_FLOOR);

    let mut offered = expand_names_to_domain_clusters(
        ["agenda:remove", "agenda:complete", "agenda:list"]
            .into_iter()
            .map(str::to_string),
        registered,
    );
    // Prefer the actionable trio first in offer order.
    for preferred in ["agenda:remove", "agenda:complete", "agenda:list"] {
        if registered.iter().any(|n| n == preferred) && !offered.iter().any(|n| n == preferred) {
            offered.insert(0, preferred.to_string());
        }
    }
    // Stable: put preferred three at front.
    let mut front = Vec::new();
    for preferred in ["agenda:remove", "agenda:complete", "agenda:list"] {
        if let Some(pos) = offered.iter().position(|n| n == preferred) {
            front.push(offered.remove(pos));
        }
    }
    front.append(&mut offered);
    offered = front;

    tracing::info!(
        suppressed_doc_hits = suppressed_doc,
        offered = ?offered,
        event = "routing.policy.agenda_dialog_pairing",
        "Agenda dialog continuation; offering agenda cluster (doc lock-in suppressed)"
    );

    Some(RoutingPolicyResult {
        offered,
        reason: "AGENDA_DIALOG_PAIRING",
    })
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

/// Prefer calling `web:fetch` this turn unless the user clearly wants opinion-only chatter.
#[must_use]
pub fn should_soft_compel_web_fetch(user_text: &str) -> bool {
    let lower = user_text.to_ascii_lowercase();
    let has_url = lower.contains("http://")
        || lower.contains("https://")
        || lower.contains("www.");
    if !has_url {
        return false;
    }
    if has_explicit_fetch_verb(&lower) {
        return true;
    }
    // Opinion-only with a URL cited as topic — still soft-compel by default so
    // "do you know this repo? https://..." fetches rather than only chatting.
    // Soft-compel is prompt bias, not a GBNF hard require.
    true
}

fn has_explicit_fetch_verb(lower: &str) -> bool {
    [
        "fetch",
        "read ",
        "read it",
        "open ",
        "open it",
        "visit ",
        "summarize",
        "look at",
        "check out",
        "pull up",
    ]
    .iter()
    .any(|v| lower.contains(v))
}

/// System-prompt block when a URL is present and web:fetch is in the offer.
pub const URL_SOFT_COMPEL_HINT: &str = "\
[URL_TOOL_HINT] The latest user message includes a URL. Prefer calling `web:fetch` this turn \
(with `web:find` afterward if you need page content) instead of only discussing the link. \
Put narration in `message_to_user` after tools return. Opinion-only replies with empty \
`tool_calls` are allowed only if the user clearly asked for your prior knowledge without reading.";

#[cfg(test)]
mod tests {
    use super::*;

    fn reg() -> Vec<String> {
        vec![
            "agenda:list".into(),
            "agenda:remove".into(),
            "agenda:complete".into(),
            "agenda:remind_at".into(),
            "agenda:push".into(),
            "clock:now".into(),
            "clock:alarm".into(),
            "clock:timer".into(),
            "doc:ingest".into(),
            "doc:query".into(),
            "doc:delete".into(),
            "doc:list".into(),
            "doc:read".into(),
            "web:fetch".into(),
            "web:find".into(),
            "web:search".into(),
            "memory:query".into(),
            "memory:commit".into(),
            "memory:stage".into(),
            "memory:staged_list".into(),
            "memory:commit_all".into(),
            "moltbook:search".into(),
            "moltbook:home".into(),
            "db:find_connections".into(),
        ]
    }

    #[test]
    fn lone_weak_doc_ingest_demoted_to_empty() {
        let hits = vec![("doc:ingest".into(), 0.505)];
        let out = apply_routing_policy("remove it", &hits, &[], &reg(), RoutingPolicyKnobs::default());
        // No recent agenda → demote to empty (full roster), not doc lock-in.
        assert!(out.offered.is_empty(), "{out:?}");
        assert_eq!(out.reason, "EMPTY_AFTER_POLICY");
    }

    #[test]
    fn lone_weak_after_agenda_pairs_to_agenda_cluster() {
        let hits = vec![("doc:ingest".into(), 0.505)];
        let recent = vec!["agenda:list".into()];
        let out = apply_routing_policy(
            "yes remove it from the agenda",
            &hits,
            &recent,
            &reg(),
            RoutingPolicyKnobs::default(),
        );
        assert_eq!(out.reason, "AGENDA_DIALOG_PAIRING");
        assert!(out.offered.iter().any(|n| n == "agenda:remove"));
        assert!(!out.offered.iter().any(|n| n.starts_with("doc:")));
    }

    #[test]
    fn strong_single_hit_kept() {
        let hits = vec![("vault:read".into(), 0.72)];
        let out =
            apply_routing_policy("read notes/today.md", &hits, &[], &reg(), RoutingPolicyKnobs::default());
        assert_eq!(out.offered, vec!["vault:read".to_string()]);
        assert_eq!(out.reason, "SINGLE_STRONG_HIT");
    }

    #[test]
    fn clock_agenda_near_tie_unions_clusters_reranked_by_cosine() {
        let hits = vec![
            ("clock:alarm".into(), 0.61),
            ("agenda:remind_at".into(), 0.59),
        ];
        let out = apply_routing_policy(
            "remind me tomorrow at 10",
            &hits,
            &[],
            &reg(),
            RoutingPolicyKnobs::default(),
        );
        assert!(out.offered.contains(&"clock:timer".to_string()));
        assert!(out.offered.contains(&"agenda:list".to_string()));
        assert!(!out.offered.contains(&"doc:ingest".to_string()));
        // Exact hits lead; no alphabetical `agenda:*` dump first.
        assert_eq!(out.offered[0], "clock:alarm");
        assert_eq!(out.offered[1], "agenda:remind_at");
    }

    #[test]
    fn unrelated_near_tie_mush_keeps_ranked_no_db_first() {
        // Replay of the live URL turn: multi-domain mush within margin of a weak top.
        let hits = vec![
            ("moltbook:search".into(), 0.586),
            ("web:search".into(), 0.569),
            ("memory:query".into(), 0.562),
            ("doc:query".into(), 0.548),
            ("memory:commit".into(), 0.543),
            ("web:fetch".into(), 0.542),
            ("db:find_connections".into(), 0.540),
            ("web:find".into(), 0.537),
        ];
        let out = apply_routing_policy(
            "do you know this repository online? https://github.com/vllm-project/semantic-router",
            &hits,
            &[],
            &reg(),
            RoutingPolicyKnobs::default(),
        );
        assert_eq!(out.offered[0], "moltbook:search");
        assert_ne!(out.offered[0], "db:find_connections");
        // No affinity union → must not expand to full db/doc/memory/moltbook/web clusters.
        assert!(
            !out.offered.iter().any(|n| n == "doc:ingest"),
            "should not dump unrelated doc cluster: {out:?}"
        );
        assert!(out.offered.iter().any(|n| n == "web:fetch"));
        let db_pos = out
            .offered
            .iter()
            .position(|n| n == "db:find_connections")
            .expect("db remains as a ranked hit");
        let fetch_pos = out
            .offered
            .iter()
            .position(|n| n == "web:fetch")
            .expect("web:fetch present");
        assert!(fetch_pos < db_pos, "cosine order: fetch before db, got {out:?}");
    }

    #[test]
    fn lexical_forced_fetch_survives_demotion() {
        let hits = vec![
            ("doc:ingest".into(), 0.505),
            ("web:fetch".into(), 1.0),
            ("web:find".into(), 0.99),
        ];
        let out = apply_routing_policy(
            "https://example.com/x",
            &hits,
            &[],
            &reg(),
            RoutingPolicyKnobs::default(),
        );
        assert!(out.offered.iter().any(|n| n == "web:fetch"));
        assert!(!out.offered.iter().any(|n| n == "doc:ingest"));
        // Cosine re-rank puts forced 1.0 first — score order, not a URL hard-pin.
        assert_eq!(out.offered[0], "web:fetch");
    }

    #[test]
    fn soft_compel_true_for_url() {
        assert!(should_soft_compel_web_fetch(
            "do you know this repo? https://github.com/vllm-project/semantic-router"
        ));
    }

    #[test]
    fn agenda_cues_and_doc_cues() {
        assert!(has_agenda_continuation_intent("done"));
        assert!(has_agenda_continuation_intent("remove that please"));
        assert!(!has_agenda_continuation_intent("what's the weather"));
        assert!(has_doc_ingest_cues("ingest report.pdf please"));
        assert!(!has_doc_ingest_cues("remove it"));
    }
}

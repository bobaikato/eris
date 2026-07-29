//! Pre-LLM tool-offer policy (Phase 1).
//!
//! Cosine hits remain the primary signal; this module demotes weak lock-in,
//! widens near-ties to domain clusters, and pairs agenda dialog continuations.

pub mod clusters;
pub mod policy;

pub use clusters::{
    affinity_group, cluster_members, domains_share_affinity, expand_names_to_domain_clusters,
    tool_domain, union_clusters_for_tools,
};
pub use policy::{
    apply_routing_policy, should_soft_compel_web_fetch, RoutingPolicyKnobs, RoutingPolicyResult,
    URL_SOFT_COMPEL_HINT,
};

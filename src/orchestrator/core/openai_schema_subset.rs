//! Per-turn JSON-Schema subset compilation for OpenRouter, mirroring
//! [`super::llama_gbnf_subset::GbnfSubsetCache`] (same cache-key-by-sorted-tool-names strategy).
//! Both constraints derive from the same `Gatekeeper` tool schemas and the same offered-tool
//! list, so GBNF and JSON-Schema subsets always offer the identical tool set.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::engine::structured::{
    build_envelope_json_schema, tool_args_schema, EnvelopeToolEntry, OpenAiSchema,
};
use crate::executive::error::{FcpError, Result};
use crate::tools::Gatekeeper;

const CACHE_KEY_NO_TOOLS: &str = "__fcp_no_tools__";

#[derive(Default)]
pub(crate) struct JsonSchemaSubsetCache {
    inner: Mutex<HashMap<String, Arc<serde_json::Value>>>,
}

impl JsonSchemaSubsetCache {
    pub(crate) fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    /// Returns cached or freshly built envelope schema for exactly `tool_names`
    /// (sorted internally for the key). Empty `tool_names` constrains `tool_calls` to `[]`.
    pub(crate) fn get_or_compile_subset(
        &self,
        gatekeeper: &Gatekeeper,
        tool_names: &[String],
    ) -> Result<Arc<serde_json::Value>> {
        let mut sorted: Vec<String> = tool_names.to_vec();
        sorted.sort();
        let key: String = if sorted.is_empty() {
            CACHE_KEY_NO_TOOLS.to_string()
        } else {
            sorted.join("\x1e")
        };

        let mut guard = self.inner.lock().map_err(|_| {
            FcpError::EngineFault("JSON-Schema subset cache mutex poisoned".to_string())
        })?;

        if let Some(hit) = guard.get(&key) {
            return Ok(Arc::clone(hit));
        }

        let entries: Vec<EnvelopeToolEntry> = sorted
            .iter()
            .map(|name| {
                let args = gatekeeper
                    .parameters_root_schema_for(name)
                    .map(|schema| tool_args_schema(name, &schema))
                    .unwrap_or_else(OpenAiSchema::empty_object);
                EnvelopeToolEntry {
                    name: name.clone(),
                    args,
                }
            })
            .collect();

        let schema = Arc::new(build_envelope_json_schema(&entries));
        guard.insert(key, Arc::clone(&schema));
        Ok(schema)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::traits::Tool;
    use async_trait::async_trait;
    use schemars::{schema_for, JsonSchema};
    use serde::Deserialize;

    #[derive(JsonSchema, Deserialize)]
    struct EmptyArgs {}

    #[derive(JsonSchema, Deserialize)]
    #[allow(dead_code)]
    struct ReadArgs {
        relative_path: String,
    }

    struct HealthStub;

    #[async_trait]
    impl Tool for HealthStub {
        fn name(&self) -> &'static str {
            "system:health"
        }
        fn description(&self) -> &'static str {
            "test"
        }
        fn parameters_schema(&self) -> schemars::schema::RootSchema {
            schema_for!(EmptyArgs)
        }
        async fn execute(&self, _args: serde_json::Value) -> crate::executive::error::Result<String> {
            Ok("{}".to_string())
        }
    }

    struct ReadStub;

    #[async_trait]
    impl Tool for ReadStub {
        fn name(&self) -> &'static str {
            "vault:read"
        }
        fn description(&self) -> &'static str {
            "test"
        }
        fn parameters_schema(&self) -> schemars::schema::RootSchema {
            schema_for!(ReadArgs)
        }
        async fn execute(&self, _args: serde_json::Value) -> crate::executive::error::Result<String> {
            Ok("{}".to_string())
        }
    }

    fn gatekeeper() -> Gatekeeper {
        let mut gk = Gatekeeper::new();
        gk.register(std::sync::Arc::new(HealthStub));
        gk.register(std::sync::Arc::new(ReadStub));
        gk
    }

    #[test]
    fn subset_lists_only_offered_tool() {
        let gk = gatekeeper();
        let cache = JsonSchemaSubsetCache::new();
        let schema = cache
            .get_or_compile_subset(&gk, &["vault:read".into()])
            .expect("subset");
        let rendered = schema.to_string();
        assert!(rendered.contains("vault:read"));
        assert!(
            !rendered.contains("system:health"),
            "subset must not include a tool omitted from the offered set"
        );
        assert!(rendered.contains("relative_path"), "typed args survive lowering");
    }

    #[test]
    fn cache_hits_return_same_arc_and_key_ignores_order() {
        let gk = gatekeeper();
        let cache = JsonSchemaSubsetCache::new();
        let a = cache
            .get_or_compile_subset(&gk, &["vault:read".into(), "system:health".into()])
            .expect("a");
        let b = cache
            .get_or_compile_subset(&gk, &["system:health".into(), "vault:read".into()])
            .expect("b");
        assert!(std::sync::Arc::ptr_eq(&a, &b), "sorted key must hit the cache");
    }

    #[test]
    fn empty_offered_set_constrains_tool_calls() {
        let gk = gatekeeper();
        let cache = JsonSchemaSubsetCache::new();
        let schema = cache.get_or_compile_subset(&gk, &[]).expect("empty subset");
        assert_eq!(schema["properties"]["tool_calls"]["maxItems"], 0);
    }

    /// GBNF and JSON-Schema subsets are built from the same offered list, so the tool sets
    /// they expose must be identical.
    #[test]
    fn gbnf_and_json_schema_subsets_offer_identical_tools() {
        let gk = gatekeeper();
        let offered = vec!["system:health".to_string(), "vault:read".to_string()];

        let gbnf_cache = super::super::llama_gbnf_subset::GbnfSubsetCache::new();
        let gbnf = gbnf_cache
            .get_or_compile_subset(&gk, &offered)
            .expect("gbnf subset");
        let json_cache = JsonSchemaSubsetCache::new();
        let json = json_cache
            .get_or_compile_subset(&gk, &offered)
            .expect("json subset");
        let json_rendered = json.to_string();

        for name in &offered {
            assert!(gbnf.contains(name.as_str()), "GBNF missing {name}");
            assert!(json_rendered.contains(name.as_str()), "JSON schema missing {name}");
        }
    }
}

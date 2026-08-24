# Tool offer cap drops write tools (persist / memorize)

Status: OPEN — analysis only (2026-08-24); do not conflate with llama.cpp long-context merge  
Log: `vaults/unknown/.fcp/telemetry/logs/fcp_core.log.2026-08-24` (after `--testrun2---`, turns ~6–9)  
Config under test: `tool_map_offer_cap = 5`, slim phrase map on

Audience: humans + AI agents working on Eris/fcp routing

---

## Symptom

User asks to **recon a site and persist** (or later: “memorize / persist findings in the vault”).
Model researches with `web:fetch` / `web:find`, then reports it **cannot** call `memory:stage` /
`vault:write` because they are “not in the toolset”. Operator sees writes in router `offered`
lists in logs and assumes a Reflect / gatekeeper bug — it is not.

## Clear picture (pipeline)

```
user text
  → ToolRouter::match_tools
  → apply_routing_policy (RANKED_SUBSET | AFFINITY_MARGIN_UNION | …)
  → RoutingDecision.matched_tool_names()     ← often INCLUDES write tools
  → apply_offer_overlays (.take(cap))        ← HERE writes often die
  → assemble_slim_tool_map / GBNF subset     ← model only sees this list
```

Routing runs **once** per user `step()`; mid-turn tool hops **reuse** the same pre-LLM offer
(`src/orchestrator/core/step.rs`). Sticky web-only offers after a fetch are a second failure mode.

### Confirmed log cases

**Turn 8 — “memorize…” (`AFFINITY_MARGIN_UNION`)**

- Semantic hits included `vault:write(0.641)`, `memory:stage(0.562)`.
- Policy union offered 10 vault+memory tools **including** writes.
- After `rerank_offered_by_cosine`, order was roughly:
  `vault:search, vault:read, vault:list, memory:query, memory:staged_list, …, vault:write, …, memory:stage`
- Slim view (`cap=5`): **only reads** —
  `vault:search, vault:read, vault:list, memory:query, memory:staged_list`
- Model called `staged_list` + `query`, then correctly said it cannot write.

**Turn 9 — “pls persist … in the vault” (`RANKED_SUBSET`)**

- Hits: `web:fetch(0.670)` … `vault:write(0.560)`, `memory:stage(0.535)`.
- Slim top-5: `web:fetch, vault:search, web:find, vault:read, web:search` — **still no writes**.
- Model kept reading vault paths; Recover widened schemas (including `vault:write` /
  `memory:stage`) but then **empty-action** Recover loops → recovery budget exhausted.

**Earlier recon turn** — affinity / ranked web cluster sticky for whole hop chain; writes never
entered the slim view even when thought said “I will memory:stage”.

## Root causes (ranked)

1. **Blind prefix take after affinity expand** (`apply_offer_overlays` in
   `src/orchestrator/routing/overlays.rs`): `.take(tool_map_offer_cap)` on cosine-ordered names.
2. **Unscored sibling inheritance** (`rerank_offered_by_cosine` in
   `src/orchestrator/routing/policy.rs`): cluster siblings that never hit embed get
   `domain_best − ε`, so e.g. `memory:staged_list` can outrank a real `vault:write` hit.
3. **Embedding bias**: “vault / memorize / persist / website” still cosine-closer to
   search/read/query than stage/write (no write-intent overlay analogous to `doc:read`→`vault:write`
   or `web:fetch`→`web:find`).
4. **Sticky mid-turn offer**: persist-after-fetch never re-routes; web top-N stays locked.

Not causes: Reflect allowlist (gatekeeper stayed `Chat`); missing write tools in registry;
GBNF “disabling” writes.

## Why raising `tool_map_offer_cap` alone is uncertain

| Cap | Likely effect |
|-----|----------------|
| 5 → 8 | Would have kept `vault:write` on turn 8 (was ~#7). Helps that case. |
| 5 → 10+ | Still fails when web+vault reads fill the prefix (turn 9 pattern), or when sticky web affinity omits memory/vault entirely. |
| 0 (uncapped) | Avoids truncate but blows slim prompt / GBNF size — defeats the improved router’s point. |

So: **band-aid yes, durable fix no.**

## Durable fix directions (pick later)

1. **Smarter top-N**: domain-diverse selection (≤k per domain) and/or reserve ≥1 mutating slot when any write tool scored above threshold.
2. **Inherit demotion**: unscored siblings must not outrank tools with real embed hits (or inherit with a larger penalty).
3. **Write-intent overlay**: lexical / policy boost for persist|memorize|stage|commit|write → ensure `memory:stage` and/or `vault:write` survive the cap (mirror `doc:read`→`vault:write`).
4. **Mid-turn re-offer**: after tool results, if user intent was multi-phase (fetch+persist) or model thought mentions stage/write, widen or re-run routing once.

## Partially addressed (2026-08-24)

The **`vision:see` → `media:catalog`** sub-case is fixed via an offer overlay in
`src/orchestrator/routing/overlays.rs` (mirrors `doc:read` → `vault:write`): whenever
`vision:see` is offered, `media:catalog` is force-appended if the state allows it, so the
remember-image flow can always reach the catalog step regardless of the cap. Covered by
`slim_offered_pairs_media_catalog_with_vision_see` in `llama_gbnf_subset.rs`. This is the
durable-fix direction #3 (write-intent overlay) applied narrowly to the vision/media flow;
the general `vault:write` / `memory:stage` cases (turns 8–9 above) remain open.

## Related

- Soak note in `docs/TODO/REFACTOR_LLAMACPP_CONTEXT_HANDLING.md` (A3 / wiki dropped by cap=5) — same class.
- `docs/HOW_TO/ADDING_A_TOOL.md` — routing layers + `tool_map_offer_cap`.
- `docs/TODO/HANDOVER-doc-summarize-v1.md` — existing `doc:read`→`vault:write` auto-offer precedent.

## Decision for now

Postpone code change until after long-context / llama.cpp merge soak is closed. Optional operator
experiment: temporarily set `tool_map_offer_cap = 8` on a vault and re-run a pure “persist this
summary with memory:stage” prompt — expect partial improvement only.

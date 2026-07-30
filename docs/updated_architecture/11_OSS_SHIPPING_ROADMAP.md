# Eris distribution roadmap

**Sole in-repo shipping contract** for getting Eris into the hands of people who do not have (and should not need) a Rust toolchain. Sync this file via git across machines. If implementation and this document disagree, **code wins** — then update this document.

Related: [10_DEEP_REVIEW_2026-07.md](./10_DEEP_REVIEW_2026-07.md) covers internal quality debt. This file covers **distribution only**.

License: **Apache 2.0** (`LICENSE`, `NOTICE`). Contributions use **DCO** sign-off (`CONTRIBUTING.md`). Copyright remains with the project author as required by Apache 2.0.

---

## 1. Goal

A stranger can install Eris with an install script, obtain every required companion, download or pull blessed models with visible trust checks, open a Markdown vault, and reach `eris chat` — **without** `cargo`, Homebrew, or reading architecture docs.

**Success bar (Linux first):** Docker available for Qdrant (or a native Qdrant binary), no Rust toolchain, `install-linux.sh` completes, backends and browser39 are on disk, models are present, first chat works.

**Sequence:** Linux install path first, then macOS. Windows later.

---

## 2. Principles

1. **Companions, not a fat binary.** Ship checksummed `eris`, `llama-server`, and `browser39` next to each other. Do not static-link llama.cpp into Eris (GPU backends differ; Eris already spawns `llama-server` as a sidecar).
2. **Docker is for Qdrant only.** Never run inference (`llama-server` / the chat model) in Docker as the supported path. On macOS, containers have no Metal; CPU inference is unacceptably slow. On Linux, prefer native/CUDA companions over a full GPU image matrix.
3. **Both backends are installable.** Ignition offers **Ollama** and **llama.cpp**. Install scripts must obtain both paths (flags to choose). llama.cpp is the canonical production backend (GBNF). Ollama is the easier model on-ramp (weaker JSON discipline).
4. **Trust is visible.** Pin URLs, tags, and sha256 (or digests). Show what is downloading and from where. No silent unpinned model pulls.
5. **No Rust for end users.** Do not require `cargo install` for browser39 or anything else on the marketed path.
6. **Soft failure beats cryptic abort.** Missing optional pieces should explain how to re-run the installer, not only how to build from source.
7. **Docs ship with the installer.** A dedicated user manual and install FAQ are part of the product, not an afterthought (see §7).

---

## 3. Runtime components

```text
                    ┌─────────────────┐
                    │  Markdown vault │  (launch cwd; .fcp/ under it)
                    └────────┬────────┘
                             │
                      ┌──────▼──────┐
                      │    eris     │  TUI and/or --web
                      └──────┬──────┘
           ┌─────────────────┼─────────────────┐
           │                 │                 │
           ▼                 ▼                 ▼
    LLM backend         Qdrant            browser39 CLI
    (pick one)       (semantic memory)   (web:* tools)
           │                 │                 │
     ┌─────┴─────┐           │                 ▼
     ▼           ▼           ▼            BROWSER39_BIN
 llama-server  ollama     Docker or        (subprocess
 + chat GGUF   + pulls    native binary     JSONL)
 + embed GGUF
```

| Component | Role | How non-devs obtain it |
|-----------|------|-------------------------|
| `eris` | Agent binary | GitHub Release via `install-linux.sh` / `install-macos.sh` |
| `llama-server` + chat/embed GGUFs | llama.cpp backend | Same release tree (or re-hosted pinned llama.cpp build) + `distribution/models.toml` |
| Ollama + model tags | Ollama backend | Official upstream install (pinned) + `ollama pull` for pinned tags |
| Qdrant | Semantic memory (default on) | `docker-compose.qdrant.yml` or Eris auto-sidecar |
| browser39 | Web fetch/search tools | Pinned prebuilt from [browser39 releases](https://github.com/alejandroqh/browser39/releases) |
| Vault | Notes / memory root | User-created Markdown folder; `cd` then `eris chat` |

Out of scope for this packaging track: vision (mmproj), voice (ffmpeg), Discord, Google Workspace, Moltbook.

### Target install layout

```text
~/.local/share/eris/
  bin/eris
  bin/browser39
  env.sh                      # PATH + BROWSER39_BIN (+ notes for config)
  llama.cpp/                  # becomes llama_cpp.home
    bin/llama-server
  models/                     # optional blessed GGUF cache
```

`env.sh` prepends `bin` to `PATH` and sets `BROWSER39_BIN`. Eris resolves the web binary via that env var or `browser39` on `PATH` (`src/tools/web/bootstrap.rs`).

Pin files (repo):

- `distribution/companions.toml` — release channel, llama.cpp tag/assets, browser39 tag/assets + sha256, Ollama installer URL/script pins
- `distribution/models.toml` — GGUF URLs + sha256; Ollama tags; **laptop** vs **comfort** model tiers (default code tags like a 26B chat model may be too heavy for first-run marketing — choose a smaller laptop default)

---

## 4. Delivery decisions

### Platforms

| Order | Path | Notes |
|-------|------|--------|
| 1 | Linux (`install-linux.sh`) | `x86_64-unknown-linux-gnu` release CI; CUDA llama-server when feasible; document CPU fallback |
| 2 | macOS (`install-macos.sh`) | Metal `llama-server`; `aarch64` / `x86_64` Apple triples already listed in `scripts/release-targets.txt` |
| Later | Windows | Optional; not in the first shipping cut |

No Homebrew tap.

### Install script contract

```bash
install-*.sh [--backend llamacpp|ollama|both]
```

Default: **`both`** (so ignition can still choose). Behavior:

1. Fetch and verify `eris`, `llama-server` (if llamacpp/both), `browser39`; write layout + `env.sh`.
2. For Ollama: detect CLI/API (`localhost:11434`); if missing, run/print the **official** installer; `ollama pull` pinned chat + embed tags with progress.
3. Optionally start Qdrant via `docker compose -f docker-compose.qdrant.yml up -d` (ports **6333** / **6334**, volume `eris-qdrant-data`).
4. Print next steps: `source env.sh`, create/cd vault, `eris chat`, link to the user manual (§7).

Keep native Qdrant + existing auto-sidecar in `src/executive/peripherals.rs`. Default `require_semantic_brain = true`; document how to disable.

### browser39

**Near term:** keep subprocess + JSONL (`docs/TODO/WEB_BROWSER39.md`). Install the CLI from upstream assets (`browser39-linux-x64`, `browser39-macos-arm64`, etc.). Soft-gate or clear reinstall messaging if missing — do not send users to `cargo install`.

**Later:** browser39 **v1.8+** is also a Rust library (`BrowserService`). Embedding it removes the external binary but requires rewriting the web fetch/consent stack. Separate milestone after Linux + macOS installers ship.

### Engines

- **llama.cpp** — recommended production (GBNF, grammar subsets, vision/voice paths).
- **Ollama** — supported end-to-end in ignition and install; document weaker structured-output guarantees.
- Do not remove Ollama until GGUF install UX fully covers the easy path *and* product explicitly wants a single backend.

Quality caveat: long-context / `n_predict` issues from the deep review remain relevant before marketing “reliable agent” without caveats. They do not block starting install scripts.

---

## 5. Privacy and download trust

Already reflected in README / `SECURITY.md`; keep true for distributors:

- No telemetry leaves the machine; `.fcp` logs are local.
- Vault data leaves only via user-configured outbound integrations.
- Prefer opt-in for Discord, GWS, Moltbook.
- Companion and model downloads are a trust surface: pin checksums, show URLs, fail closed on hash mismatch.

---

## 6. Milestones

### M1 — Linux installable

| ID | Work |
|----|------|
| L1 | Soft-gate / honest errors for missing browser39 (point at installer) |
| L2 | `docker-compose.qdrant.yml` + README one-liner |
| L3 | `distribution/companions.toml` + `distribution/models.toml` |
| L4 | Linux release CI (`scripts/build-release-targets.sh`); `eris` + `SHA256SUMS` on GitHub Releases |
| L5 | Linux `llama-server` asset (CUDA and/or CPU), checksummed |
| L6 | `install-linux.sh` (companions + Ollama + optional Qdrant + `env.sh`) |
| L7 | Align `setup_welder` preflight (green/red) with install layout |
| L8 | **User manual + install FAQ** (§7) — first edition, Linux-complete |
| L9 | README non-dev quickstart (no `cargo`) linking to the manual |

### M2 — macOS installable

Same contract: Metal `llama-server`, macOS browser39 assets, official Ollama macOS path, extend the user manual with Gatekeeper / Metal sections. Explicit: do not put `llama-server` in Docker on Mac.

### M3 — Polish

- Keep the user manual current with every installer change.
- `config_version` + migration for `AppConfig`.
- Demo GIF; release notes that list exact companion pins.
- Drift-prevention from the deep review before inviting large external tool PRs.

### M4 — Later

- Embed browser39 as a crate; drop CLI companion from install when stable.
- Windows.
- Optional vault path UX (`eris chat ~/Notes`).
- Single-backend decision only if deliberate.

---

## 7. User manual and install FAQ (required deliverable)

Existing [`docs/HOW_TO/END_USER_README.md`](../HOW_TO/END_USER_README.md) assumes a lone pre-built `eris` binary. That is insufficient for the companion model. Ship a dedicated manual that matches what the installer actually does.

**Proposed path:** `docs/HOW_TO/INSTALL_AND_USER_MANUAL.md` (name flexible; update links from README when it exists).

The manual is written for humans, not agents. This roadmap section defines **what it must contain** so implementers write it like a real distributor: install path first, then day-one use, then failure modes.

### 7.1 Manual outline (must cover)

1. **What you are installing** — Eris + companions diagram (one page); what stays local; Apache 2.0.
2. **Hardware expectations** — RAM/VRAM table (laptop tier vs comfort tier); Apple Silicon vs Linux NVIDIA vs CPU-only honesty.
3. **Prerequisites** — Docker (or native Qdrant); disk space for models; network for first download; no Rust required.
4. **Install (Linux)** — copy-paste `install-linux.sh` invocation; `--backend` flags explained; what gets written under `~/.local/share/eris/`.
5. **Install (macOS)** — same, plus Gatekeeper / quarantine notes.
6. **Environment** — `source ~/.local/share/eris/env.sh` (or shell profile snippet); how to verify `which eris`, `browser39 --version`, `llama-server --version` / Ollama API.
7. **Models** — laptop vs comfort; how re-download / verify checksums works; approximate sizes and time.
8. **Qdrant** — compose up/down; ports; volume name; when Eris auto-starts a sidecar instead.
9. **First vault** — create folder, `cd`, `eris chat`, ignition backend choice, what `.fcp/` means.
10. **Day-one use** — TUI basics, `eris chat --web`, clean `/exit` (reap peripherals), where logs live.
11. **Upgrading** — re-run installer; pin versions; do not mix unmatched `eris` / `llama-server` / browser39 pins.
12. **Uninstall / reset** — what to delete; Qdrant volume; vault vs install tree.
13. **Privacy** — short pointer to `SECURITY.md`.
14. **Get help** — GitHub issues template fields (OS, arch, installer log, checksum output, backend choice).

Operator-depth llama.cpp tuning stays in [`LLAMA_CPP_SETUP.md`](../HOW_TO/LLAMA_CPP_SETUP.md); the manual links there instead of duplicating CMake builds.

### 7.2 Install FAQ — think ahead (seed content for the manual)

Write each entry as: **Symptom → Likely cause → Fix**. Expand as real tickets arrive; start with this matrix.

| Symptom | Likely cause | Fix |
|---------|--------------|-----|
| Installer exits on checksum mismatch | Corrupt download, CDN glitch, or pin file out of date | Re-run download; compare published `SHA256SUMS`; do not `--ignore-hash` |
| `cannot execute binary file` / Exec format error | Wrong arch asset (e.g. x64 on arm64) | Re-run installer for detected arch; check `uname -m` |
| macOS “app can’t be opened” / killed on launch | Gatekeeper quarantine | Documented `xattr` clear for **user-trusted** Release assets only; never bypass for random mirrors |
| `eris: command not found` after install | `env.sh` not sourced; PATH not updated | `source ~/.local/share/eris/env.sh`; optional profile line |
| Chat aborts mentioning browser39 | Binary missing or `BROWSER39_BIN` unset | Re-run installer browser39 step; confirm `env.sh`; soft-gate messaging should point here |
| “Semantic brain” / Qdrant required failure | Docker daemon down, compose not started, port busy | Start Docker; `docker compose … up -d`; check 6333/6334; or document `require_semantic_brain = false` for vault-only experiments |
| `docker: permission denied` | User not in `docker` group (Linux) | Distro-specific group membership; re-login |
| Port already in use (6333/6334, 11434, llama ports) | Stale Qdrant/Ollama/llama-server from a previous crash | Document how to find/kill orphans; prefer `/exit` in Eris; list default ports in the manual |
| Ollama install succeeded but API unreachable | `ollama serve` not running | Start service; Eris may spawn serve if CLI on PATH — confirm with curl to `:11434` |
| `ollama pull` fails / stalls | Network, disk full, or tag typo vs pin file | Disk check; retry; verify tag against `distribution/models.toml` |
| GGUF download huge / machine swaps or OOM | Comfort-tier model on laptop RAM | Switch to laptop-tier pins; lower GPU layers / smaller quant |
| llama-server starts then dies | Bad model path, insufficient VRAM, wrong binary (CUDA vs CPU) | Preflight paths; match asset to GPU; read `.fcp` logs |
| Very slow replies on Mac | Inference accidentally under Docker/VM or CPU-only binary | Use Metal-native `llama-server` from installer; never Dockerize llama on Mac |
| CUDA llama-server fails on Linux | Driver / toolkit mismatch | Document minimum driver expectation; offer CPU asset as fallback |
| Ignition can’t find llama-server | `llama_cpp.home` not pointing at install tree | Set home to `~/.local/share/eris/llama.cpp` (or whatever installer wrote) |
| Ran `eris chat` in the wrong directory | Vault is cwd-based | `cd` into vault; explain `.fcp/` appears where you launched |
| Partial install / half-upgraded mix | Interrupted script; manual file copies | Installer should be **idempotent**; FAQ says “re-run full install”; never mix pins across releases |
| Corporate proxy / TLS intercept | Downloads fail or hash weirdness | Document proxy env vars; offline/airgap is best-effort later |
| Disk filled by models + Qdrant volume | Large GGUFs + vector store | Sizes in manual; how to delete `models/` and compose volume |
| Web tools fail after chat starts | Allowlist, consent wall, or browser39 too old vs pin | Link web how-to; upgrade browser39 via installer |
| “It worked yesterday” after OS update | Quarantine re-applied, Docker Desktop reset, PATH lost | Re-source `env.sh`; re-check Docker; re-run installer |

### 7.3 Distributor habits (process)

- Every installer PR updates the manual and FAQ in the **same** change when behavior or pins move.
- Release notes list exact pins: Eris version, llama.cpp tag, browser39 tag, Ollama installer pin, model tier hashes/tags.
- Prefer idempotent installs and a single “verify” command or welder table users can paste into issues.
- Do not send non-dev users into `docs/updated_architecture/` for install failures.

---

## 8. Implementation checklist

Update checkboxes here when work lands:

- [ ] Soft-gate browser39 / honest missing-binary messaging
- [ ] `docker-compose.qdrant.yml` + docs
- [ ] `distribution/companions.toml`
- [ ] `distribution/models.toml` (laptop + comfort tiers)
- [ ] `install-linux.sh` (eris, llama-server, browser39, checksums, `env.sh`)
- [ ] `install-linux.sh` Ollama path (`--backend`)
- [ ] Linux release CI + `SHA256SUMS`
- [ ] Welder preflight aligned with install layout
- [ ] **`docs/HOW_TO/INSTALL_AND_USER_MANUAL.md`** (outline §7.1 + FAQ §7.2)
- [ ] README non-dev quickstart → links to manual
- [ ] `install-macos.sh` + Metal companions + manual macOS sections
- [ ] *(Later)* Embed browser39 library; rewrite web fetch

### PR split

1. Soft-gate + README honesty  
2. Qdrant compose  
3. Pin manifests  
4. `install-linux.sh`  
5. Linux release CI  
6. Welder alignment  
7. User manual + FAQ (can start in parallel once layout is stable)  
8. macOS install + manual updates  
9. *(Later)* browser39 crate embed  

---

## 9. Code pointers

| Concern | Location |
|---------|----------|
| Project laws | `.cursorrules` |
| Spawn/reap llama-server, Ollama, Qdrant | `src/executive/peripherals.rs` |
| Backend choice (ignition) | `src/executive/ignition.rs` |
| Setup welder probes | `src/executive/setup_welder/` |
| browser39 resolve/probe | `src/tools/web/bootstrap.rs` |
| Web fetch (JSONL subprocess) | `docs/TODO/WEB_BROWSER39.md`, `src/tools/web/` |
| Chat startup web stack | `src/executive/chat_session.rs` |
| Config defaults | `src/config.rs` |
| Release build | `scripts/build-release-targets.sh`, `scripts/release-targets.txt` |
| Legacy end-user notes | `docs/HOW_TO/END_USER_README.md` (supersede via §7 manual) |
| llama.cpp operator depth | `docs/HOW_TO/LLAMA_CPP_SETUP.md` |
| CI today | `.github/workflows/ci.yml` (no release artifacts yet) |

---

## 10. Non-goals

- Homebrew  
- Windows in M1/M2  
- Static-linking llama.cpp into `eris`  
- Full-stack Docker (eris + llama + qdrant)  
- Packaging vision / voice / Discord / GWS  
- Changing vault-cwd semantics in the install track  
- Embedding the browser39 library before Linux + macOS installers ship  

---

## 11. Notes for the implementing agent

1. Build **install scripts + release CI + user manual**, not “Dockerize Eris.”
2. **Ollama is in scope** because ignition offers it.
3. **browser39** = upstream prebuilt CLI in M1/M2; library embed is M4.
4. **Linux before macOS.**
5. Do not `git add` / `git commit` unless the human asks.
6. Extend `setup_welder`; do not invent a second onboarding system.
7. Keep **this file** and the **user manual** updated when the shipping contract changes.

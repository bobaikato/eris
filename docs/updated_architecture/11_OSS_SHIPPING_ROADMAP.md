# Eris distribution roadmap

**Sole in-repo shipping contract** for getting Eris into the hands of people who do not have (and should not need) a Rust toolchain. Sync this file via git across machines. If implementation and this document disagree, **code wins** — then update this document.

Related: [10_DEEP_REVIEW_2026-07.md](./10_DEEP_REVIEW_2026-07.md) covers internal quality debt. This file covers **distribution only**.

License: **Apache 2.0** (`LICENSE`, `NOTICE`). Contributions use **DCO** sign-off (`CONTRIBUTING.md`). Copyright remains with the project author as required by Apache 2.0.

---

## 0. Decisions locked (from design Q&A)

These supersede older draft wording elsewhere in this file if anything drifts.

| Topic | Decision |
|-------|----------|
| Default chat model | **Gemma 4 E4B**, **Q4_K_M** GGUF (`unsloth/gemma-4-E4B-it-GGUF`); Ollama tag **`gemma4:e4b`**. See `distribution/models.toml` |
| Default embed model | **`nomic-embed-text-v1.5.Q8_0.gguf`** / Ollama tag **`nomic-embed-text`** |
| Other models | Prominent hint in installer next-steps, ignition, and manual — not a full model-picker UI in M1 |
| Hardware floor | Marketed path assumes a **GPU with ≥ ~8GB VRAM**, or **Apple Silicon** with comparable unified memory. CPU-only is **not** a first-class install target |
| Linux `llama-server` | Ship **Vulkan** build as the default GPU companion (NVIDIA + AMD + Intel with working Vulkan drivers). Broader than CUDA; one binary story |
| CUDA / other GPU APIs | **Not** a TOML enum. Eris has no `backend = "vulkan"`. Switch = replace the binary under `llama_cpp.home` (or retarget `home`) + adjust `n_gpu_layers`. Document on **eris-site FAQ** + in-repo manual |
| macOS `llama-server` | **Metal** only. Never Dockerize inference on Mac |
| `llama-server` provenance | Prefer **pinned upstream or Eris-built assets** with sha256 in `companions.toml` (fetch-by-pin). Official llama.cpp ships Linux **CPU** and macOS well; **Linux Vulkan** is an official `ubuntu-vulkan-*` asset on ggml-org releases |
| Artifact store | **GitHub Releases** hold binaries + pin files + `SHA256SUMS` |
| Human front door | **eris-site** hosts the install one-liner / marketing copy and FAQ (including Vulkan→CUDA swap). Site does **not** need to host multi‑GB GGUFs |
| browser39 | Installer downloads upstream **prebuilt CLI**, sets `BROWSER39_BIN`. No `cargo install` on the marketed path. Library embed = M4 |
| Pin SSOT | **Option B:** `distribution/companions.toml` + `distribution/models.toml` are the only human-edited pin sources. `install-*.sh` hardcodes **repo + release tag** (and maybe arch); at runtime it downloads those two TOMLs + `SHA256SUMS` from the **same** GitHub Release and uses them. Do **not** dual-maintain sha256 inside the shell script long-term |
| Checksums / release CI | Release pipeline builds `eris`, packs the tarball, writes `SHA256SUMS`, uploads tarball + pin TOMLs + installer + `SHA256SUMS`. Local compile hash = **dry-run only** until the workflow exists |
| Eris binary hash today | Still open until a Release asset exists; local `sha256sum` of the packed tarball is the interim dry-run pin |
| Vault seeding | Still done by first `eris chat` / ignition (`.fcp/`, skills, web operator files). Installer does **not** invent a second vault bootstrap |
| Ignition prep | **In scope / prerequisite:** when `~/.local/share/eris/` layout exists, ignition must default `llama_cpp.home` and model paths there — not `~/llama.cpp/build` |
| Web UI sidebar | Does **not** edit `llama_cpp.home` / model paths / `n_gpu_layers` today. v1 switch path = edit vault `config.toml` (+ binary). Backend settings panel = later product choice |
| Target ladder | **M1a** `x86_64-unknown-linux-gnu` → **M1b** `aarch64-unknown-linux-gnu` → **M2** Apple (`aarch64-apple-darwin`, then Intel Mac if still needed) |
| Ollama | Remains installable (`--backend`); ignition still offers it |

---

## 1. Goal

A stranger can install Eris with an install script, obtain every required companion, download or pull blessed models with visible trust checks, open a Markdown vault, and reach `eris chat` — **without** `cargo`, Homebrew, or reading architecture docs.

**Success bar (Linux first):** Docker available for Qdrant (or a native Qdrant binary), no Rust toolchain, `install-linux.sh` completes, **Vulkan** `llama-server` + browser39 + blessed models on disk, first chat works on a machine meeting the **≥8GB VRAM** (or equivalent) floor.

**Sequence:** Linux install path first, then macOS. Windows later.

---

## 2. Principles

1. **Companions, not a fat binary.** Ship checksummed `eris`, `llama-server`, and `browser39` next to each other. Do not static-link llama.cpp into Eris (GPU backends differ; Eris already spawns `llama-server` as a sidecar).
2. **Docker is for Qdrant only.** Never run inference (`llama-server` / the chat model) in Docker as the supported path. On macOS, containers have no Metal. On Linux, ship a **native Vulkan** companion — not a full GPU Docker image matrix.
3. **Both LLM backends are installable.** Ignition offers **Ollama** and **llama.cpp**. Install scripts obtain both paths (flags to choose). llama.cpp is the canonical production backend (GBNF). Ollama is the easier model on-ramp (weaker JSON discipline).
4. **Trust is visible.** Pin URLs, tags, and sha256 (or digests). Show what is downloading and from where. No silent unpinned model pulls.
5. **No Rust for end users.** Do not require `cargo install` for browser39 or anything else on the marketed path.
6. **Soft failure beats cryptic abort.** Missing pieces explain how to re-run the installer, not only how to build from source. browser39 messaging must not say `cargo install`.
7. **Docs ship with the installer.** In-repo user manual + install FAQ are required; **eris-site** mirrors the human front door and the Vulkan→other-backend FAQ.
8. **Local GPU product.** Eris is for machines with real local acceleration (≥8GB-class VRAM or Apple Silicon). Do not market stone-age CPU-only as supported first chat.
9. **GPU API = the binary.** Config points at a tree (`llama_cpp.home`); it does not select Vulkan vs CUDA vs Metal by string.
10. **Pin SSOT = Option B (§5).** Humans edit `distribution/*.toml` only; release CI publishes them + `SHA256SUMS`; installer fetches those from the Release.

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
 (Vulkan/Metal
  default binary)
```

| Component | Role | How non-devs obtain it |
|-----------|------|-------------------------|
| `eris` | Agent binary | GitHub Release via `install-linux.sh` / `install-macos.sh`; discover via eris-site |
| `llama-server` + chat/embed GGUFs | llama.cpp backend | Pinned companion (Linux **Vulkan** / macOS **Metal**) + `distribution/models.toml` |
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
    bin/llama-server          # Linux: Vulkan build; macOS: Metal build
    # (+ shared libs as required by that build)
  models/                     # blessed GGUF cache (chat + embed)
```

`env.sh` prepends `bin` to `PATH` and sets `BROWSER39_BIN`. Eris resolves the web binary via that env var or `browser39` on `PATH` (`src/tools/web/bootstrap.rs`).

Pin files (repo → also published on each GitHub Release; see **§5**):

- `distribution/companions.toml` — Eris release channel; llama.cpp tag/asset (**Vulkan** Linux, **Metal** macOS) + sha256; browser39 tag/assets + sha256; Ollama installer URL/script pins; optional later CUDA asset pin for power users
- `distribution/models.toml` — GGUF URLs + sha256; Ollama tags; **default** = Gemma 4 E4B Q4 + nomic embed; optional **comfort** tier called out as upgrade, not first-run default
- `SHA256SUMS` — produced by release CI for **Eris-built** artifacts on that Release (at minimum the `eris-*.tar.gz`); companion/model hashes live in the TOMLs

### Two knobs users confuse (document clearly)

| Knob | Meaning | Where |
|------|---------|--------|
| Which `llama-server` binary | Vulkan vs CUDA vs Metal vs CPU | Files under `llama_cpp.home` (installer default = Vulkan on Linux, Metal on Mac) |
| `n_gpu_layers` | How many layers that binary may offload | Vault `.fcp/config.toml` → `[llama_cpp]` |

A CUDA binary with `n_gpu_layers = 0` can still run on CPU; a Vulkan/Metal binary does not become CUDA by editing TOML alone.

### Hosting split

| Surface | Role |
|---------|------|
| **GitHub Releases** (this repo) | Artifact store: `eris` tarball, `SHA256SUMS`, published `companions.toml` / `models.toml`, `install-*.sh` |
| **eris-site** | Human front door: `curl … \| bash` one-liner (tagged installer URL), hardware expectations, FAQ |

---

## 4. Delivery decisions

### Platforms (target ladder)

| Order | Path | Notes |
|-------|------|--------|
| M1a | Linux `x86_64-unknown-linux-gnu` | First public cut; Vulkan `llama-server`; enable triple in CI / `scripts/release-targets.txt` |
| M1b | Linux `aarch64-unknown-linux-gnu` | Same installer, different assets (Graviton, Asahi, ARM boxes) |
| M2 | macOS (`install-macos.sh`) | Metal `llama-server`; `aarch64-apple-darwin` first; `x86_64-apple-darwin` if still needed |
| Later | Windows | Not in M1/M2 |

No Homebrew tap.

### Hardware expectations (marketed)

- **Linux:** GPU with **≥ ~8GB VRAM** and working **Vulkan** drivers (NVIDIA / AMD / Intel).
- **macOS:** Apple Silicon (unified memory); Metal build. Intel Mac only if we ship that triple.
- **Not marketed:** CPU-only first chat. If mentioned at all: unsupported / best-effort for developers.

### Install script contract

```bash
install-*.sh [--backend llamacpp|ollama|both]
```

Default: **`both`** (so ignition can still choose). Behavior:

1. Detect arch; fetch and verify `eris`, **Vulkan** (Linux) / **Metal** (macOS) `llama-server` (+ libs), `browser39`; write layout + `env.sh`.
2. Fetch and verify blessed GGUFs into `models/` (or document Ollama pulls for the Ollama path).
3. For Ollama: detect CLI/API (`localhost:11434`); if missing, run/print the **official** installer; `ollama pull` pinned chat + embed tags with progress.
4. Optionally start Qdrant via `docker compose -f docker-compose.qdrant.yml up -d` (ports **6333** / **6334**, volume `eris-qdrant-data`). Soft-fail with clear Docker instructions if daemon missing.
5. Print next steps: `source env.sh`, create/cd vault, `eris chat`, link to manual + eris-site FAQ; note hardware floor and that shipped `llama-server` is Vulkan (Linux) / Metal (macOS).

Keep native Qdrant + existing auto-sidecar in `src/executive/peripherals.rs`. Default `require_semantic_brain = true`; document how to disable.

### Ignition / config prep (prerequisite, in this track)

Today ignition defaults `llama.cpp` home toward `~/llama.cpp/build` and asks for free-form GGUF paths (`src/executive/ignition.rs`). After a non-dev install that is wrong.

**Required before or with the first real installer PR:**

1. If `~/.local/share/eris/llama.cpp/bin/llama-server` exists, **default** `llama_cpp.home` to that tree (accept with Enter).
2. If blessed GGUFs exist under `~/.local/share/eris/models/`, **default** chat/embed paths to those files.
3. Default `n_gpu_layers` appropriate for GPU offload (e.g. `99`), not CPU-oriented `0`.
4. Vault seeding (skills, `.fcp/browser39/`, seal, etc.) stays in ignition / first chat — installer only drops the share tree + `env.sh`.

Web UI tools sidebar does **not** need to grow llama path editors for M1.

### browser39

**Near term:** keep subprocess + JSONL (`docs/TODO/WEB_BROWSER39.md`). Installer downloads upstream CLI assets (`browser39-linux-x64`, `browser39-macos-arm64`, etc.), installs under `~/.local/share/eris/bin/`, sets `BROWSER39_BIN`, prints verified version. Soft-gate / honest errors if missing later → re-run installer — **never** `cargo install`.

**Later (M4):** browser39 as Rust library (`BrowserService`); rewrite web fetch/consent stack; drop CLI companion when stable.

### Engines

- **llama.cpp** — recommended production (GBNF, grammar subsets). Shipped companion = Vulkan (Linux) / Metal (macOS).
- **Ollama** — supported end-to-end in ignition and install; document weaker structured-output guarantees.
- Do not remove Ollama until GGUF install UX fully covers the easy path *and* product explicitly wants a single backend.

Quality caveat: long-context / `n_predict` issues from the deep review remain relevant before marketing “reliable agent” without caveats. They do not block starting install scripts.

### Switching off Vulkan (userland FAQ — eris-site + manual)

Eris does **not** expose `backend = "vulkan"`. To use CUDA (or another build):

1. Obtain a CUDA (or other) `llama-server` build (+ required libs).
2. Replace files under `~/.local/share/eris/llama.cpp/` **or** point vault config at another tree:

   ```toml
   [llama_cpp]
   home = "/path/to/other/llama.cpp/build"   # must contain bin/llama-server
   n_gpu_layers = 99
   ```

3. Restart `eris chat`.

Optional later installer flag (`--llama-backend cuda`) only after a pinned CUDA asset exists — not required for M1a.

---

## 5. Pin SSOT, release pipeline, and GitHub CLI (`gh`)

This section is the shipping contract for **where checksums live**, **how the installer learns them**, and **how operators dry-run or inspect Releases**. Implement the release workflow and installer fetch path after the local dry-run pin; **docs-first here**.

### 5.1 Single source of truth (Option B — locked)

| Layer | What | Who edits |
|-------|------|-----------|
| **SSOT in git** | `distribution/companions.toml`, `distribution/models.toml` | Humans (and agents under human review). Companion/model URL + sha256 live **only** here |
| **Release artifact list** | Same two TOMLs **copied onto the GitHub Release** for that tag, plus `SHA256SUMS` for Eris-built binaries | Release workflow |
| **Installer script** | Hardcodes `ERIS_REPO` + `ERIS_TAG` (and arch detection). Does **not** embed a second copy of every sha256 long-term | Bootstrap only |

**Runtime (Option B):**

1. User runs `install-linux.sh` (from eris-site one-liner or Release asset).
2. Script downloads from  
   `https://github.com/<repo>/releases/download/<tag>/`  
   → `companions.toml`, `models.toml`, `SHA256SUMS` (and then the binaries those files describe).
3. Script parses pins from the downloaded TOMLs; verifies Eris tarball (and any other Release-hosted blobs) against `SHA256SUMS`; verifies upstream companion/model downloads against sha256 fields inside the TOMLs.
4. Fail closed on mismatch. No `--ignore-hash`.

**Anti-pattern (current interim):** duplicating every hash as shell variables inside `install-linux.sh`. Acceptable only as a **bridge** until Option B lands; then delete the mirrored pin block.

**Chicken-egg note:** the installer script itself is fetched unpinned on first `curl | bash` (same trust model as most OSS installers). After that, everything it pulls for that tag is pinned. Prefer publishing `install-linux.sh` on the Release and pointing eris-site at that exact tag URL.

### 5.2 What the release pipeline must produce

Add `.github/workflows/release.yml` (name flexible) triggered on version tags (`v*`), roughly:

1. Build `eris` for `x86_64-unknown-linux-gnu` (later: matrix aarch64 / macOS).
2. Pack `eris-x86_64-unknown-linux-gnu.tar.gz` with `eris` at archive root (matches installer extract).
3. `sha256sum` → write `SHA256SUMS` (standard format: `<hash>  <filename>` per line).
4. Upload to the GitHub Release for that tag:
   - `eris-x86_64-unknown-linux-gnu.tar.gz`
   - `SHA256SUMS`
   - `companions.toml` (from `distribution/companions.toml`, with `[eris]` tag/asset fields aligned to this Release)
   - `models.toml` (from `distribution/models.toml`)
   - `install-linux.sh`
5. Release notes list the tag, Vulkan llama pin, browser39 pin, model pins.

The **fitting hash for the Eris binary is whatever the pipeline hashed for the file it uploaded** — not a hash from a different machine’s build unless you are deliberately dry-running.

Until this workflow exists: local pack + `sha256sum` is a **dry-run** only (see §5.4).

### 5.3 Inspecting checksums with GitHub CLI (`gh`)

Requires [GitHub CLI](https://cli.github.com/) authenticated to the repo (`gh auth status`).

```bash
# List releases / pick a tag
gh release list --repo janpauldahlke/eris

# List assets on a tag
gh release view v0.1.1-alpha --repo janpauldahlke/eris

# Download pin files + checksum list from that Release (into cwd)
gh release download v0.1.1-alpha --repo janpauldahlke/eris \
  --pattern 'SHA256SUMS' \
  --pattern 'companions.toml' \
  --pattern 'models.toml' \
  --pattern 'eris-*.tar.gz'

# Show published Eris hashes
cat SHA256SUMS

# Verify a downloaded tarball against SHA256SUMS
sha256sum -c SHA256SUMS --ignore-missing
```

API alternative (no `gh`): Release asset digests appear on  
`https://api.github.com/repos/janpauldahlke/eris/releases/tags/<tag>`  
under each asset’s `digest` field (`sha256:…`) once assets exist.

Upstream companions (browser39, llama.cpp) already expose digests the same way on *their* Releases; those values are copied into `companions.toml` when pinning, not re-derived by Eris CI.

### 5.4 Local dry-run (before release CI)

**Operator-friendly walkthrough (copy-paste, pitfalls, `gh` upload):**  
[`docs/HOW_TO/RELEASING.md`](../HOW_TO/RELEASING.md)

Use the dry-run only to unblock pinning / sanity-check pack layout. Prefer the pipeline hash once `release.yml` ships.

**Pitfall:** `cargo build` produces `target/x86_64-unknown-linux-gnu/release/eris`. That is **not** `eris-x86_64-unknown-linux-gnu.tar.gz`. If `sha256sum eris-….tar.gz` says “No such file”, you still need to **pack** first:

```bash
# From repo root, on Linux x86_64 (or with the target installed):
rustup target add x86_64-unknown-linux-gnu   # once
cargo build --release --target x86_64-unknown-linux-gnu

mkdir -p dist/dry-run
cp -f target/x86_64-unknown-linux-gnu/release/eris dist/dry-run/eris
tar -C dist/dry-run -czf dist/eris-x86_64-unknown-linux-gnu.tar.gz eris

# Must list exactly: eris
tar -tzf dist/eris-x86_64-unknown-linux-gnu.tar.gz

sha256sum dist/eris-x86_64-unknown-linux-gnu.tar.gz
# → paste the hex into distribution/companions.toml [eris.assets.linux-x86_64]
#   (and install-linux.sh mirror until Option B / R2)
# Optional local checksum file (same format CI will publish):
#   (cd dist && sha256sum eris-x86_64-unknown-linux-gnu.tar.gz > SHA256SUMS)
```

Archive contract: tarball contains **`eris`** at the root (no nested folder), because `install-linux.sh` runs  
`tar -xzf … -C "$BIN_DIR" eris`.

Hash the **`.tar.gz`**, not only the bare binary under `target/`.

### 5.5 Follow-up work (implement after dry-run)

| ID | Work |
|----|------|
| R1 | `.github/workflows/release.yml` — build, pack, `SHA256SUMS`, upload assets listed in §5.2 |
| R2 | Refactor `install-linux.sh` to Option B (fetch TOMLs + `SHA256SUMS`; strip embedded pin mirror) |
| R3 | Ensure `[eris]` in published `companions.toml` matches the tag/asset names on that Release |
| R4 | eris-site one-liner points at a **tagged** `install-linux.sh` URL, not an unpinned `main` blob |
| R5 | Document operator habit: pin upstream companions in git TOMLs; never hand-edit hashes inside the shell script |

---

## 6. Privacy and download trust

Already reflected in README / `SECURITY.md`; keep true for distributors:

- No telemetry leaves the machine; `.fcp` logs are local.
- Vault data leaves only via user-configured outbound integrations.
- Prefer opt-in for Discord, GWS, Moltbook.
- Companion and model downloads are a trust surface: pin checksums, show URLs, fail closed on hash mismatch.

---

## 7. Milestones (detailed)

### M0 — Ignition + messaging prep (do first)

| ID | Work | Detail |
|----|------|--------|
| P0 | Ignition defaults for install layout | Prefer `~/.local/share/eris/llama.cpp` and `models/` when present; stop forcing DIY `~/llama.cpp/build` for that case |
| P1 | browser39 honest errors | Replace `cargo install` hints in `src/tools/web/bootstrap.rs` (and docs that claim it) with installer / `BROWSER39_BIN` / re-run install guidance |
| P2 | README honesty | Non-dev path must not imply lone binary + cargo browser39; point forward to manual when it exists |

### M1 — Linux installable

| ID | Work | Detail |
|----|------|--------|
| L1 | Soft-gate follow-through | Chat/web startup messages align with P1; welder can probe browser39 + install tree |
| L2 | `docker-compose.qdrant.yml` | Ports 6333/6334, volume `eris-qdrant-data`; README / manual one-liner; soft-fail if Docker missing |
| L3 | `distribution/companions.toml` | Pin Eris release pattern; Linux **Vulkan** llama-server asset + sha256; browser39 linux assets + sha256; Ollama installer pin |
| L4 | `distribution/models.toml` | Default: Gemma 4 E4B Q4 chat URL+sha256; nomic embed; Ollama tags; comfort tier; “other models” hint |
| L5 | Linux release CI (**R1**, §5.2) | Build `x86_64` `eris`; upload tarball + `SHA256SUMS` + pin TOMLs + `install-linux.sh` |
| L6 | Vulkan `llama-server` asset | Pin checksummed Vulkan linux-x64 pack in `companions.toml`; document driver expectation |
| L7 | `install-linux.sh` (**R2**, §5.1 Option B) | Fetch `companions.toml` + `models.toml` + `SHA256SUMS` from Release; hardcode repo+tag only; idempotent install |
| L8 | Ollama path in installer | Official install + pinned pulls when `--backend ollama\|both`; soft-fail + manual hint after retries |
| L9 | Welder alignment | Green/red probes understand `~/.local/share/eris/` layout and Vulkan home |
| L10 | User manual + FAQ (§8) | Linux-complete first edition; include Vulkan→CUDA swap via `llama_cpp.home` |
| L11 | README non-dev quickstart | No `cargo`; link manual; mention eris-site one-liner when live |
| L12 | eris-site front door | Install one-liner → GitHub Release script/assets; FAQ: hardware ≥8GB; Vulkan default; how to swap binary / `home` |
| L13 | M1b aarch64 | Same contract, aarch64 assets (eris + Vulkan companion + browser39) |

### M2 — macOS installable

| ID | Work | Detail |
|----|------|--------|
| A1 | `install-macos.sh` | Same layout under `~/.local/share/eris/` (or documented macOS equivalent if we ever diverge — prefer same) |
| A2 | Metal `llama-server` | Pinned Metal build; never Docker for inference |
| A3 | browser39 macOS assets | arm64 (and x64 if shipping Intel) |
| A4 | Gatekeeper / quarantine | Manual + FAQ `xattr` only for user-trusted Release assets |
| A5 | Manual + eris-site | Metal / unified-memory sections; Ollama macOS path |

### M3 — Polish

- Keep manual + eris-site FAQ current with every installer/pin change.
- `config_version` + migration for `AppConfig`.
- Demo GIF; release notes list exact companion pins (Eris, llama tag/backend, browser39, model hashes).
- Drift-prevention from the deep review before inviting large external tool PRs.
- Optional: installer `--llama-backend cuda` once a CUDA asset is pinned.

### M4 — Later

- Embed browser39 as a crate; drop CLI companion from install when stable.
- Windows.
- Optional vault path UX (`eris chat ~/Notes`).
- Single-backend decision only if deliberate.
- Optional web UI panel for `llama_cpp` paths / layers (not required for shipping).

---

## 8. User manual and install FAQ (required deliverable)

Existing [`docs/HOW_TO/END_USER_README.md`](../HOW_TO/END_USER_README.md) assumes a lone pre-built `eris` binary. That is insufficient for the companion model. Ship a dedicated manual that matches what the installer actually does.

**Proposed path:** `docs/HOW_TO/INSTALL_AND_USER_MANUAL.md` (name flexible; update links from README when it exists). Mirror hardware + Vulkan FAQ on **eris-site**.

### 8.1 Manual outline (must cover)

1. **What you are installing** — Eris + companions diagram; what stays local; Apache 2.0.
2. **Hardware expectations** — **≥ ~8GB VRAM** (or Apple Silicon); Linux Vulkan drivers; CPU-only not supported on marketed path.
3. **Prerequisites** — Docker (or native Qdrant); disk for models; network for first download; no Rust required.
4. **Install (Linux)** — copy-paste install invocation (site or raw Release URL); `--backend` flags; layout under `~/.local/share/eris/`; shipped companion is **Vulkan**.
5. **Install (macOS)** — same + Gatekeeper; shipped companion is **Metal**.
6. **Environment** — `source ~/.local/share/eris/env.sh`; verify `which eris`, `browser39 --version`, `llama-server --version` / Ollama API.
7. **Models** — default Gemma ~4B Q4 + nomic embed; comfort / other models hint; checksum re-verify.
8. **Changing GPU backend** — no TOML enum; replace binary or retarget `llama_cpp.home`; set `n_gpu_layers`; restart chat. Link eris-site FAQ.
9. **Qdrant** — compose up/down; ports; volume; auto-sidecar behavior.
10. **First vault** — create folder, `cd`, `eris chat`, ignition (defaults should hit install tree), what `.fcp/` means.
11. **Day-one use** — TUI, `eris chat --web`, clean `/exit`, logs under `.fcp/`.
12. **Upgrading** — re-run installer; do not mix unmatched pins.
13. **Uninstall / reset** — share tree vs vault vs Qdrant volume.
14. **Privacy** — pointer to `SECURITY.md`.
15. **Get help** — issue fields: OS, arch, GPU/driver, installer log, checksum output, backend choice, `llama_cpp.home`.

Operator-depth llama.cpp tuning stays in [`LLAMA_CPP_SETUP.md`](../HOW_TO/LLAMA_CPP_SETUP.md); the manual links there instead of duplicating CMake builds.

### 8.2 Install FAQ — seed matrix

Write each entry as: **Symptom → Likely cause → Fix**.

| Symptom | Likely cause | Fix |
|---------|--------------|-----|
| Installer exits on checksum mismatch | Corrupt download or stale pin | Re-run; compare Release `SHA256SUMS`; no `--ignore-hash` |
| `cannot execute binary file` | Wrong arch asset | Re-run for `uname -m` |
| macOS “can’t be opened” / killed | Gatekeeper quarantine | Documented `xattr` for **trusted** Release assets only |
| `eris: command not found` | `env.sh` not sourced | `source ~/.local/share/eris/env.sh` |
| Chat aborts on browser39 | Missing binary / unset `BROWSER39_BIN` | Re-run installer; confirm `env.sh` — never cargo |
| Semantic brain / Qdrant failure | Docker down, ports busy | compose up; or `require_semantic_brain = false` for experiments |
| `docker: permission denied` | Not in `docker` group | Distro docs; re-login |
| Port in use (6333/6334, 11434, 8090/8091) | Orphan peripherals | Prefer `/exit`; document how to clear |
| Ollama API unreachable | `ollama serve` not running | Start service; curl `:11434` |
| `ollama pull` fails | Network / disk / wrong tag | Check disk; verify `models.toml` |
| OOM / thrash on load | Model too large for VRAM | Stay on 4B Q4 default; lower layers; see comfort-tier warning |
| llama-server starts then dies | Bad paths, VRAM, driver, wrong binary | Match Vulkan build to Vulkan drivers; read `.fcp` logs |
| Want CUDA instead of Vulkan | Different binary required | Install CUDA `llama-server`; replace under share tree or change `llama_cpp.home`; set `n_gpu_layers`; restart |
| Slow on Mac | Docker/VM or non-Metal binary | Metal companion only; never Dockerize llama |
| Ignition asks for `~/llama.cpp/build` | Install tree not detected / P0 not done | Ensure share layout exists; ignition should default to it after P0 |
| Wrong cwd | Vault is cwd-based | `cd` into vault |
| Partial / mixed pins | Interrupted install | Re-run full idempotent install |
| Proxy / TLS intercept | Hash or download failures | Document proxy env; airgap later |
| Disk full | GGUFs + Qdrant volume | Sizes in manual; how to delete |
| Web tools fail mid-session | Allowlist / consent / old browser39 | Web how-to; re-run installer for CLI |
| “Worked yesterday” | PATH / Docker / quarantine drift | Re-source `env.sh`; re-check Docker; re-run installer |

### 8.3 Distributor habits

- Every installer PR updates the manual (and eris-site FAQ when behavior users see changes) in the **same** change when behavior or pins move.
- Release notes list exact pins: Eris version, llama.cpp tag **and GPU API** (Vulkan/Metal/CUDA), browser39 tag, Ollama installer pin, model hashes/tags.
- Prefer idempotent installs and a welder/verify table users can paste into issues.
- Do not send non-dev users into `docs/updated_architecture/` for install failures.

---

## 9. Implementation checklist

Update checkboxes here when work lands:

- [ ] **P0** Ignition defaults for `~/.local/share/eris/` layout
- [ ] **P1** Soft-gate browser39 / no `cargo install` on marketed path
- [ ] **P2** README honesty for non-dev path
- [ ] `docker-compose.qdrant.yml` + docs
- [ ] `distribution/companions.toml` (Vulkan Linux + browser39 + Ollama pins)
- [ ] `distribution/models.toml` (Gemma 4 E4B Q4 + nomic embed; comfort optional)
- [ ] Local dry-run pack + sha256 for `eris-*.tar.gz` (§5.4) — interim only
- [ ] **R1** Release workflow: build, pack, `SHA256SUMS`, upload TOMLs + installer (§5.2)
- [ ] **R2** `install-linux.sh` Option B: fetch `companions.toml` + `models.toml` + `SHA256SUMS` from Release (§5.1)
- [ ] Vulkan `llama-server` asset pinned (upstream b10189 or successor)
- [ ] Welder preflight aligned with install layout
- [ ] **`docs/HOW_TO/INSTALL_AND_USER_MANUAL.md`** (§8) including Vulkan→CUDA via `home`
- [ ] eris-site one-liner + FAQ (hardware floor, Vulkan default, swap recipe); tagged installer URL (§5.5 R4)
- [ ] README non-dev quickstart → manual / site
- [ ] `install-macos.sh` + Metal companions + manual macOS sections
- [ ] *(Later)* CUDA optional asset / installer flag
- [ ] *(Later)* Embed browser39 library; rewrite web fetch

### PR split

1. **Ignition install-layout defaults (P0)**  
2. Soft-gate browser39 + README honesty (P1/P2)  
3. Qdrant compose  
4. Pin manifests (`companions.toml` / `models.toml`) — SSOT only  
5. Local dry-run eris tarball hash (interim)  
6. **Release CI (R1)** + publish `SHA256SUMS` + pin TOMLs on the Release  
7. **Installer Option B (R2)** — fetch pins from Release; drop shell hash mirror  
8. Welder alignment  
9. User manual + FAQ (parallel once layout stable)  
10. eris-site front door + FAQ mirror  
11. aarch64 Linux assets  
12. macOS install + Metal + manual/site updates  
13. *(Later)* CUDA optional / browser39 crate embed  

---

## 10. Code pointers

| Concern | Location |
|---------|----------|
| Project laws | `.cursorrules` |
| Spawn/reap llama-server, Ollama, Qdrant | `src/executive/peripherals.rs` |
| Backend choice + path prompts (ignition) | `src/executive/ignition.rs` |
| Setup welder probes | `src/executive/setup_welder/` |
| browser39 resolve/probe | `src/tools/web/bootstrap.rs` |
| Web fetch (JSONL subprocess) | `docs/TODO/WEB_BROWSER39.md`, `src/tools/web/` |
| Chat startup web stack | `src/executive/chat_session.rs` |
| Config / `[llama_cpp]` | `src/config.rs` |
| Web tools sidebar (no llama home editors today) | `src/ui/web/tools_config_schema.rs` |
| Pin SSOT | `distribution/companions.toml`, `distribution/models.toml` |
| Installer (interim embedded pins → Option B) | `install-linux.sh` |
| **How to pack / hash / `gh` release (operators)** | [`docs/HOW_TO/RELEASING.md`](../HOW_TO/RELEASING.md) |
| Release build helper | `scripts/build-release-targets.sh`, `scripts/release-targets.txt` |
| Release workflow (to add) | `.github/workflows/release.yml` (§5.2) |
| Legacy end-user notes | `docs/HOW_TO/END_USER_README.md` (supersede via §8 manual) |
| llama.cpp operator depth | `docs/HOW_TO/LLAMA_CPP_SETUP.md` |
| CI today | `.github/workflows/ci.yml` (tests only; no release artifacts yet) |
| Front door (sibling) | `eris-site` (install one-liner + FAQ; not in this repo) |

---

## 11. Non-goals

- Homebrew  
- Windows in M1/M2  
- Static-linking llama.cpp into `eris`  
- Full-stack Docker (eris + llama + qdrant)  
- Marketing CPU-only as a supported first-run path  
- Shipping CUDA as the **default** Linux companion (Vulkan is default; CUDA is documented swap / optional later)  
- A `backend = "vulkan"` config key (API is the binary)  
- Dual-maintaining sha256 in both TOML and `install-*.sh` after Option B lands  
- Packaging vision / voice / Discord / GWS  
- Changing vault-cwd semantics in the install track  
- Embedding the browser39 library before Linux + macOS installers ship  
- Web UI editors for `llama_cpp.home` in M1  

---

## 12. Notes for the implementing agent

1. Build **install scripts + release CI + user manual (+ eris-site FAQ copy)**, not “Dockerize Eris.”
2. **Ollama is in scope** because ignition offers it.
3. **browser39** = upstream prebuilt CLI in M1/M2; library embed is M4.
4. **Linux before macOS.** Within Linux: **x86_64 then aarch64**.
5. **Vulkan default** on Linux; **Metal** on macOS; CUDA only as documented binary swap unless a later optional asset lands.
6. **P0 ignition defaults** before or with the first installer — otherwise non-devs hit DIY paths after a successful install.
7. **Pin SSOT = Option B (§5).** Do not keep a permanent second hash table in the shell script. Release CI owns `SHA256SUMS` for Eris-built artifacts.
8. Local `sha256sum` of a tarball is a **dry-run** until the release workflow uploads the matching asset.
9. Do not `git add` / `git commit` unless the human asks.
10. Extend `setup_welder`; do not invent a second onboarding system.
11. Keep **this file**, the **user manual**, and **eris-site FAQ** updated when the shipping contract changes.
12. Do not invent checksums — copy from upstream Release digests, HF LFS oid, or `SHA256SUMS` produced by our pipeline.

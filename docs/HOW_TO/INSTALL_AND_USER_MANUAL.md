# Eris — Install and User Manual (Linux)

This is the manual for people who install Eris with the install script and do **not** have (or want) a Rust toolchain. It matches what `install-linux.sh` actually does. macOS sections arrive with the macOS installer (M2); until then this manual is Linux-complete.

Operator-depth llama.cpp tuning (CMake builds, custom flags) lives in [`LLAMA_CPP_SETUP.md`](./LLAMA_CPP_SETUP.md) — you do not need it for a normal install.

---

## 1. What you are installing

Eris is a local agent that lives in a Markdown vault (a plain folder of notes). The installer places Eris and its companions side by side — no fat binary, no Docker image of everything:

```text
                    ┌─────────────────┐
                    │  Markdown vault │  (your folder; .fcp/ appears under it)
                    └────────┬────────┘
                             │
                      ┌──────▼──────┐
                      │    eris     │  TUI and/or --web
                      └──────┬──────┘
           ┌─────────────────┼─────────────────┐
           ▼                 ▼                 ▼
    LLM backend         Qdrant            browser39 CLI
    (llama.cpp or    (semantic memory,   (web tools,
     Ollama)          Docker)             subprocess)
```

Everything runs on your machine. No telemetry leaves it; logs stay under your vault's `.fcp/` directory. License: Apache 2.0.

## 2. Hardware expectations

- A GPU with **≥ ~8GB VRAM** and **working Vulkan drivers** (NVIDIA, AMD, or Intel — `vulkaninfo --summary` should succeed).
- CPU-only first chat is **not** a supported install target. It may limp along; we do not market or debug it.
- Disk: roughly 3GB for the default models, plus the Qdrant volume as your memory grows.

## 3. Prerequisites

- **Docker** with the compose plugin (for Qdrant only — inference never runs in Docker). A native Qdrant binary works too if you prefer.
- `curl`, `tar`, `unzip`, `sha256sum` (all standard on mainstream distros).
- Network for the first download.
- **No Rust, no cargo, no Homebrew.** If any instruction tells you to `cargo install` something, it is outdated — re-run the installer instead.

## 4. Install (Linux)

Get the one-liner from the eris-site front door, or run the script from the GitHub Release directly:

```bash
curl -fsSL https://github.com/GITHUB_OWNER/eris/releases/download/vX.Y.Z/install-linux.sh | bash
```

Flags (when running the script directly):

```bash
./install-linux.sh --backend llamacpp   # llama.cpp only (canonical production backend)
./install-linux.sh --backend ollama     # Ollama only (easier model on-ramp)
./install-linux.sh --backend both       # default; ignition still lets you choose
./install-linux.sh --no-qdrant          # skip starting Qdrant now
```

x86_64 only for now; aarch64 Linux and macOS follow on the roadmap.

What lands on disk:

```text
~/.local/share/eris/
  bin/eris
  bin/browser39
  env.sh                      # PATH + BROWSER39_BIN + config notes
  llama.cpp/                  # this is your llama_cpp.home
    bin/llama-server          # VULKAN build (the Linux default)
  models/                     # checksum-verified GGUFs (chat + embed)
  docker-compose.qdrant.yml
  cache/                      # verified downloads (safe to delete)
```

Trust model: every artifact is pinned by URL and sha256 in `distribution/companions.toml` and `distribution/models.toml`. The installer shows what it downloads and from where, and **fails closed** on any checksum mismatch. There is no override flag. On mismatch it deletes the file and tells you to re-run.

Re-running the installer is always safe: it verifies what exists and repairs what does not.

## 5. Install (macOS)

Coming with M2 (`install-macos.sh`, Metal `llama-server`, Gatekeeper notes). Not available yet.

## 6. Environment

```bash
source ~/.local/share/eris/env.sh     # add to ~/.bashrc / ~/.zshrc to persist
```

Verify:

```bash
which eris                                          # → ~/.local/share/eris/bin/eris
browser39 --version                                 # web tools CLI answers
~/.local/share/eris/llama.cpp/bin/llama-server --version
curl -s http://localhost:11434/api/version          # only if using Ollama
```

`eris: command not found` almost always means `env.sh` was not sourced in this shell.

## 7. Models

Defaults (pinned in `distribution/models.toml`, verified by the installer):

| Role | Model | File / Ollama tag |
|------|-------|--------------------|
| Chat | Gemma 3 4B instruct, Q4 (~2.5GB) | `gemma-3-4b-it-Q4_K_M.gguf` / `gemma3:4b` |
| Embeddings | nomic-embed-text v1.5 Q8_0 | `nomic-embed-text-v1.5.Q8_0.gguf` / `nomic-embed-text` |

**Comfort tier (optional upgrade, not the default):** Gemma 3 12B Q4 (~7.3GB) if you have roughly 12GB+ VRAM. If it loads and then thrashes or OOMs, go back to 4B.

**Other models:** Eris runs any chat GGUF that fits your VRAM. Drop the `.gguf` into `~/.local/share/eris/models/` and point your vault's `.fcp/config.toml` at it (`[llama_cpp] chat_model_path`), or `ollama pull <tag>` on the Ollama backend. Only the pinned defaults are checksum-verified — anything else you vet yourself. Leave the embed model alone unless you know the vector width matches your Qdrant collection.

Re-verify a model at any time:

```bash
sha256sum ~/.local/share/eris/models/*.gguf   # compare with distribution/models.toml
```

## 8. Changing GPU backend (Vulkan → CUDA or other)

Two knobs people confuse:

| Knob | Meaning | Where |
|------|---------|-------|
| Which `llama-server` binary | Vulkan vs CUDA vs Metal vs CPU | Files under `llama_cpp.home` |
| `n_gpu_layers` | How many layers that binary offloads | Vault `.fcp/config.toml` → `[llama_cpp]` |

There is **no** `backend = "vulkan"` config key. The GPU API *is* the binary. A CUDA binary with `n_gpu_layers = 0` still runs on CPU; a Vulkan binary does not become CUDA by editing TOML.

To switch to CUDA (or anything else):

1. Obtain a CUDA `llama-server` build plus its required libs.
2. Either replace the files under `~/.local/share/eris/llama.cpp/bin/`, **or** point your vault at another tree:

   ```toml
   [llama_cpp]
   home = "/path/to/other/llama.cpp/build"   # must contain bin/llama-server
   n_gpu_layers = 99
   ```

3. Restart `eris chat`.

The eris-site FAQ mirrors this recipe.

## 9. Qdrant (semantic memory)

Started by the installer via Docker. Ports **6333** (HTTP) and **6334** (gRPC — Eris connects here), data in the `eris-qdrant-data` volume.

```bash
docker compose -f ~/.local/share/eris/docker-compose.qdrant.yml up -d    # start
docker compose -f ~/.local/share/eris/docker-compose.qdrant.yml down     # stop (data kept)
```

If Qdrant is unreachable, chat startup can offer to launch it (auto-sidecar). By default Eris **requires** the semantic brain; to experiment without it, set `require_semantic_brain = false` in the vault's `.fcp/config.toml` — not recommended for real use.

## 10. First vault

```bash
mkdir -p ~/eris-vault
cd ~/eris-vault
eris chat
```

The vault is whatever folder you launch from — being in the right directory matters. First run starts **ignition**, which walks you through backend choice and paths. With the install tree present, ignition defaults to it: `llama_cpp.home = ~/.local/share/eris/llama.cpp`, the blessed models under `models/`, and GPU offload (`n_gpu_layers = 99`). Accept the defaults with Enter.

Ignition seeds `.fcp/` under your vault: config, skills, logs, web operator files. That directory belongs to Eris; your Markdown stays yours.

## 11. Day-one use

- `eris chat` — the TUI.
- `eris chat --web` — same agent plus the web UI.
- Leave with `/exit` — this cleanly shuts down the llama servers and releases VRAM. Killing the terminal instead is how you end up with orphaned processes and busy ports.
- Logs live under `<vault>/.fcp/` — that is the first place to look (and the first thing to attach to an issue).

## 12. Upgrading

Re-run the installer. It is idempotent and moves all pins together (Eris, llama-server, browser39, models). Do **not** hand-mix versions — a new `eris` with a stale companion set is an unsupported state. Release notes list the exact pins each version ships.

## 13. Uninstall / reset

Three independent things:

| What | Remove with | Loses |
|------|-------------|-------|
| Program + companions + models | `rm -rf ~/.local/share/eris` (and drop the `source env.sh` line from your shell rc) | Nothing of yours |
| A vault's Eris state | `rm -rf <vault>/.fcp` | That vault's config, logs, seeded skills — your Markdown is untouched |
| Semantic memory | `docker compose -f ~/.local/share/eris/docker-compose.qdrant.yml down -v` | Everything Eris learned into Qdrant |

## 14. Privacy

No telemetry. Nothing leaves your machine except through outbound integrations you explicitly configure. Downloads are pinned and verified. Details: [`SECURITY.md`](../../SECURITY.md).

## 15. Install FAQ (symptom → likely cause → fix)

| Symptom | Likely cause | Fix |
|---------|--------------|-----|
| Installer exits on checksum mismatch | Corrupt download or stale pin | Re-run; compare the Release `SHA256SUMS`. There is no `--ignore-hash`, by design |
| Installer aborts on "placeholder pin" | Script copy predates human pinning | Get the script from the actual GitHub Release, not a work-in-progress branch |
| `cannot execute binary file` | Wrong arch asset | Check `uname -m`; only x86_64 is shipped right now |
| `eris: command not found` | `env.sh` not sourced | `source ~/.local/share/eris/env.sh`; add to shell rc |
| Chat aborts on browser39 | Missing binary / unset `BROWSER39_BIN` | Re-run the installer; confirm `env.sh` is sourced. Never `cargo install` |
| Semantic brain / Qdrant failure | Docker down or ports busy | `docker compose ... up -d`; or `require_semantic_brain = false` for experiments only |
| `docker: permission denied` | Not in the `docker` group | Add yourself per distro docs; log out and back in |
| Port in use (6333/6334, 11434, 8090/8091) | Orphaned peripherals from a hard exit | Prefer `/exit`; find and stop the orphan (`ss -ltnp`), then restart chat |
| Ollama API unreachable | `ollama serve` not running | Start the service; `curl localhost:11434/api/version` |
| `ollama pull` fails | Network / disk / wrong tag | Check disk space; use the tags from `distribution/models.toml` |
| OOM or thrash on model load | Model too large for VRAM | Stay on the 4B Q4 default; lower `n_gpu_layers`; see the comfort-tier warning |
| llama-server starts then dies | Bad paths, VRAM, driver, wrong binary | The Vulkan binary needs Vulkan drivers (`vulkaninfo --summary`); read `.fcp/` logs |
| Want CUDA instead of Vulkan | Different binary required | §8 above: swap the binary or retarget `llama_cpp.home`; set `n_gpu_layers`; restart |
| Ignition suggests `~/llama.cpp/build` | Install tree not detected | Confirm `~/.local/share/eris/llama.cpp/bin/llama-server` exists; re-run installer |
| Eris acts like the vault is empty | Launched from the wrong directory | `cd` into the vault, then `eris chat` |
| Partial / mixed pins | Interrupted install | Re-run the full installer; it repairs in place |
| Hash or download failures behind a proxy | TLS interception | Set your proxy env vars; a MITM proxy that rewrites bodies will (correctly) fail checksums |
| Disk full | GGUFs + Qdrant volume | §13 for what is safe to delete; models are ~3GB, comfort tier ~7GB more |
| Web tools fail mid-session | Allowlist / consent / stale browser39 | See the web how-to; re-run the installer to refresh the CLI |
| "Worked yesterday" | PATH / Docker / env drift | Re-source `env.sh`; check Docker is up; re-run the installer |

## 16. Get help

Open a GitHub issue and include: OS and distro, `uname -m`, GPU model and driver (`vulkaninfo --summary` output), which backend you chose, the installer output, `sha256sum` output for the failing artifact if relevant, your `llama_cpp.home` value, and the tail of the newest log under `<vault>/.fcp/`.

You should never need `docs/updated_architecture/` to fix an install problem — if this manual failed you, that is a bug in the manual; say so in the issue.

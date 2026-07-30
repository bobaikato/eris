#!/usr/bin/env bash
# =============================================================================
# Eris Linux installer (roadmap L7 / R2 — docs/updated_architecture/11_OSS_SHIPPING_ROADMAP.md)
#
#   curl -fsSL https://github.com/GITHUB_OWNER/eris/releases/download/vX.Y.Z/install-linux.sh | bash
#   # or: ./install-linux.sh [--backend llamacpp|ollama|both] [--no-qdrant]
#
# Pin model — Option B (roadmap §5.1):
#   * This script hardcodes ONLY the repo + release tag below. It does NOT
#     carry a second copy of every sha256.
#   * At runtime it downloads companions.toml, models.toml, and SHA256SUMS
#     from that SAME GitHub Release, then reads all pins from them.
#   * The Eris tarball is verified against SHA256SUMS (produced by release CI).
#     Companion + model downloads are verified against the sha256 fields inside
#     the downloaded TOMLs.
#   * Chicken-egg: this script itself is fetched unpinned on first curl|bash
#     (same trust model as most OSS installers). Everything it pulls for the
#     pinned tag afterwards IS verified.
#
# Contract:
#   * Installs Eris + companions under ~/.local/share/eris/ (no root, no cargo,
#     no Homebrew, no Rust toolchain).
#   * Linux companion llama-server is the VULKAN build (NVIDIA/AMD/Intel with
#     working Vulkan drivers). CUDA = documented binary swap, see the manual.
#   * Every download is sha256-verified and FAILS CLOSED on mismatch. There is
#     no --ignore-hash. Placeholder pins abort before anything is downloaded.
#   * Idempotent: re-running verifies/repairs; already-good artifacts are kept.
#   * Docker is used for Qdrant ONLY. Inference is never Dockerized.
# =============================================================================
set -euo pipefail

# ─── Bootstrap pins (the ONLY hardcoded pins — Option B) ─────────────────────
# Everything else is read at runtime from the pin manifests published on the
# Release for ERIS_TAG below.
#
# ERIS_TAG channels:
#   * dist-*  = distribution/testing channel (GitHub pre-release). Re-cut freely
#               to exercise the pipeline. This is what we point at while testing.
#   * v*      = real semver milestone for the marketed install. Flip ERIS_TAG to
#               the v-tag (e.g. "v0.1.1-alpha") once a real release is published.

ERIS_REPO="janpauldahlke/eris"
ERIS_TAG="dist-test"

# ─── Layout ──────────────────────────────────────────────────────────────────

ERIS_HOME="${HOME}/.local/share/eris"
BIN_DIR="${ERIS_HOME}/bin"
LLAMA_HOME="${ERIS_HOME}/llama.cpp"          # becomes [llama_cpp] home
MODELS_DIR="${ERIS_HOME}/models"
ENV_FILE="${ERIS_HOME}/env.sh"
COMPOSE_FILE="${ERIS_HOME}/docker-compose.qdrant.yml"

BACKEND="both"          # llamacpp | ollama | both
WANT_QDRANT=1

# ─── Helpers ─────────────────────────────────────────────────────────────────

log()  { printf '\033[1;34m[eris-install]\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m[eris-install] WARNING:\033[0m %s\n' "$*" >&2; }
die()  { printf '\033[1;31m[eris-install] ERROR:\033[0m %s\n' "$*" >&2; exit 1; }

usage() {
  cat <<'EOF'
Eris Linux installer

Usage: install-linux.sh [options]

Options:
  --backend llamacpp|ollama|both   LLM backend(s) to set up (default: both).
                                   Ignition still lets you choose per vault.
  --no-qdrant                      Skip starting Qdrant via docker compose.
  -h, --help                       Show this help.

Hardware floor: a GPU with >= ~8GB VRAM and working Vulkan drivers.
CPU-only first chat is not a supported install target.
EOF
}

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || die "required command '$1' not found. $2"
}

# Fail closed if a pin is still a placeholder or came back empty. Never download
# something we cannot verify.
require_pinned() {
  local name="$1" value="$2"
  if [ -z "$value" ]; then
    die "pin '${name}' is empty. The manifest downloaded from the Release is
missing it, or the section/key name changed. Refusing to download anything
unverifiable."
  fi
  case "$value" in
    *SHA256_HERE*|*DIGEST_HERE*|*vX.Y.Z*|*bXXXX*|*GITHUB_OWNER*)
      die "pin '${name}' is still a placeholder (${value}).
The published manifest was not finalized before release. Refusing to download
anything unverifiable."
      ;;
  esac
}

# toml_get FILE SECTION KEY — print the value of KEY inside [SECTION].
# Minimal reader for the flat TOML we publish (string / bare scalar values,
# inline comments and surrounding quotes stripped). No external deps.
toml_get() {
  local file="$1" section="$2" key="$3"
  awk -v section="$section" -v key="$key" '
    function trim(s){ sub(/^[[:space:]]+/,"",s); sub(/[[:space:]]+$/,"",s); return s }
    /^[[:space:]]*#/ { next }
    /^[[:space:]]*\[/ {
      h=$0; sub(/^[[:space:]]*\[/,"",h); sub(/\].*$/,"",h); cur=trim(h); next
    }
    {
      if (cur==section && $0 ~ ("^[[:space:]]*" key "[[:space:]]*=")) {
        v=$0; sub(/^[^=]*=/,"",v); v=trim(v)
        if (substr(v,1,1)=="\"") {
          v=substr(v,2); idx=index(v,"\""); if (idx>0) v=substr(v,1,idx-1)
        } else {
          sub(/[[:space:]#].*$/,"",v)
        }
        print v; exit
      }
    }
  ' "$file"
}

# sha_from_sums SUMSFILE FILENAME — print the expected hash for FILENAME from a
# standard `<hash>  <filename>` SHA256SUMS file (handles the binary '*' prefix).
sha_from_sums() {
  local sums="$1" fname="$2"
  awk -v f="$fname" '$2==f || $2=="*"f {print $1; exit}' "$sums"
}

# verify_sha256 FILE EXPECTED — fail closed on mismatch.
verify_sha256() {
  local file="$1" expected="$2" actual
  actual="$(sha256sum "$file" | awk '{print $1}')"
  if [ "$actual" != "$expected" ]; then
    rm -f "$file"
    die "checksum mismatch for $(basename "$file")
  expected: ${expected}
  actual:   ${actual}
The corrupted file was deleted. Re-run this installer. If it fails again,
compare against SHA256SUMS on the GitHub Release — the pin may be stale.
There is no way to skip this check, by design."
  fi
}

# fetch_unverified URL DEST LABEL — plain download (used ONLY for the pin
# manifests themselves; see chicken-egg note in the header).
fetch_unverified() {
  local url="$1" dest="$2" label="$3"
  log "${label}: downloading pin manifest"
  log "  from: ${url}"
  curl --proto '=https' --tlsv1.2 -fL --progress-bar -o "${dest}.part" "$url" \
    || die "could not download ${label} from the Release (${url}).
Check that ${ERIS_TAG} exists and has assets, and check your network."
  mv "${dest}.part" "$dest"
}

# fetch_verified URL DEST EXPECTED_SHA256 LABEL — idempotent pinned download.
fetch_verified() {
  local url="$1" dest="$2" expected="$3" label="$4"
  if [ -f "$dest" ]; then
    if [ "$(sha256sum "$dest" | awk '{print $1}')" = "$expected" ]; then
      log "${label}: already present and verified, skipping download."
      return 0
    fi
    warn "${label}: existing file failed verification; re-downloading."
    rm -f "$dest"
  fi
  log "${label}: downloading"
  log "  from: ${url}"
  log "  to:   ${dest}"
  curl --proto '=https' --tlsv1.2 -fL --progress-bar -o "${dest}.part" "$url" \
    || die "download failed for ${label}. Check your network and re-run."
  mv "${dest}.part" "$dest"
  verify_sha256 "$dest" "$expected"
  log "${label}: sha256 verified."
}

# ─── Arg parsing ─────────────────────────────────────────────────────────────

while [ $# -gt 0 ]; do
  case "$1" in
    --backend)
      shift
      BACKEND="${1:-}"
      case "$BACKEND" in llamacpp|ollama|both) ;; *) die "--backend must be llamacpp, ollama, or both" ;; esac
      ;;
    --no-qdrant) WANT_QDRANT=0 ;;
    -h|--help) usage; exit 0 ;;
    *) die "unknown option: $1 (see --help)" ;;
  esac
  shift
done

# ─── Preflight ───────────────────────────────────────────────────────────────

need_cmd curl "Install it via your distro (e.g. apt install curl)."
need_cmd sha256sum "Part of coreutils on every mainstream distro."
need_cmd tar "Part of every mainstream distro."
need_cmd unzip "Install it via your distro (e.g. apt install unzip)."
need_cmd awk "Part of every mainstream distro."

ARCH="$(uname -m)"
case "$ARCH" in
  x86_64) ;;
  aarch64|arm64)
    die "aarch64 Linux assets are not published yet (roadmap M1b).
x86_64 is the only supported Linux target right now." ;;
  *) die "unsupported architecture: ${ARCH}. Supported: x86_64." ;;
esac

require_pinned "eris.repo" "$ERIS_REPO"
require_pinned "eris.release_tag" "$ERIS_TAG"

mkdir -p "$BIN_DIR" "$MODELS_DIR" "${LLAMA_HOME}/bin" "${ERIS_HOME}/cache"
CACHE="${ERIS_HOME}/cache"
REL_BASE="https://github.com/${ERIS_REPO}/releases/download/${ERIS_TAG}"

# ─── 0. Fetch pin manifests from the Release, then read every pin from them ──

log "Reading pins for ${ERIS_REPO} ${ERIS_TAG} from its GitHub Release."
COMPANIONS_TOML="${CACHE}/companions.toml"
MODELS_TOML="${CACHE}/models.toml"
SHA256SUMS_FILE="${CACHE}/SHA256SUMS"
fetch_unverified "${REL_BASE}/companions.toml" "$COMPANIONS_TOML" "companions.toml"
fetch_unverified "${REL_BASE}/models.toml"     "$MODELS_TOML"     "models.toml"
fetch_unverified "${REL_BASE}/SHA256SUMS"      "$SHA256SUMS_FILE" "SHA256SUMS"

# Eris tarball: name from companions.toml, hash from SHA256SUMS (CI-produced).
ERIS_ASSET_X86_64="$(toml_get "$COMPANIONS_TOML" "eris.assets.linux-x86_64" "asset")"
require_pinned "eris.assets.linux-x86_64.asset" "$ERIS_ASSET_X86_64"
ERIS_SHA256_X86_64="$(sha_from_sums "$SHA256SUMS_FILE" "$ERIS_ASSET_X86_64")"
require_pinned "SHA256SUMS[${ERIS_ASSET_X86_64}]" "$ERIS_SHA256_X86_64"

# browser39 (always installed).
BROWSER39_REPO="$(toml_get "$COMPANIONS_TOML" "browser39" "repo")"
BROWSER39_TAG="$(toml_get "$COMPANIONS_TOML" "browser39" "tag")"
BROWSER39_ASSET_X86_64="$(toml_get "$COMPANIONS_TOML" "browser39.assets.linux-x86_64" "asset")"
BROWSER39_SHA256_X86_64="$(toml_get "$COMPANIONS_TOML" "browser39.assets.linux-x86_64" "sha256")"
require_pinned "browser39.repo" "$BROWSER39_REPO"
require_pinned "browser39.tag" "$BROWSER39_TAG"
require_pinned "browser39.assets.linux-x86_64.asset" "$BROWSER39_ASSET_X86_64"
require_pinned "browser39.assets.linux-x86_64.sha256" "$BROWSER39_SHA256_X86_64"

# llama.cpp Vulkan + blessed models (llamacpp / both).
if [ "$BACKEND" != "ollama" ]; then
  LLAMA_TAG="$(toml_get "$COMPANIONS_TOML" "llama_cpp" "tag")"
  LLAMA_VULKAN_URL_X86_64="$(toml_get "$COMPANIONS_TOML" "llama_cpp.assets.linux-x86_64-vulkan" "url")"
  LLAMA_VULKAN_SHA256_X86_64="$(toml_get "$COMPANIONS_TOML" "llama_cpp.assets.linux-x86_64-vulkan" "sha256")"
  require_pinned "llama_cpp.tag" "$LLAMA_TAG"
  require_pinned "llama_cpp.assets.linux-x86_64-vulkan.url" "$LLAMA_VULKAN_URL_X86_64"
  require_pinned "llama_cpp.assets.linux-x86_64-vulkan.sha256" "$LLAMA_VULKAN_SHA256_X86_64"

  CHAT_MODEL_FILE="$(toml_get "$MODELS_TOML" "default.chat" "filename")"
  CHAT_MODEL_URL="$(toml_get "$MODELS_TOML" "default.chat" "url")"
  CHAT_MODEL_SHA256="$(toml_get "$MODELS_TOML" "default.chat" "sha256")"
  EMBED_MODEL_FILE="$(toml_get "$MODELS_TOML" "default.embed" "filename")"
  EMBED_MODEL_URL="$(toml_get "$MODELS_TOML" "default.embed" "url")"
  EMBED_MODEL_SHA256="$(toml_get "$MODELS_TOML" "default.embed" "sha256")"
  require_pinned "models.default.chat.filename" "$CHAT_MODEL_FILE"
  require_pinned "models.default.chat.url" "$CHAT_MODEL_URL"
  require_pinned "models.default.chat.sha256" "$CHAT_MODEL_SHA256"
  require_pinned "models.default.embed.filename" "$EMBED_MODEL_FILE"
  require_pinned "models.default.embed.url" "$EMBED_MODEL_URL"
  require_pinned "models.default.embed.sha256" "$EMBED_MODEL_SHA256"
fi

# Ollama installer + pinned tags (ollama / both).
CHAT_OLLAMA_TAG="$(toml_get "$MODELS_TOML" "default.chat" "ollama_tag")"
EMBED_OLLAMA_TAG="$(toml_get "$MODELS_TOML" "default.embed" "ollama_tag")"
if [ "$BACKEND" != "llamacpp" ]; then
  OLLAMA_INSTALL_URL="$(toml_get "$COMPANIONS_TOML" "ollama" "install_script_url")"
  OLLAMA_INSTALL_SHA256="$(toml_get "$COMPANIONS_TOML" "ollama" "install_script_sha256")"
  require_pinned "ollama.install_script_url" "$OLLAMA_INSTALL_URL"
  require_pinned "ollama.install_script_sha256" "$OLLAMA_INSTALL_SHA256"
  require_pinned "models.default.chat.ollama_tag" "$CHAT_OLLAMA_TAG"
  require_pinned "models.default.embed.ollama_tag" "$EMBED_OLLAMA_TAG"
fi

# Qdrant image ref: image:tag@digest (pinned by manifest digest).
if [ "$WANT_QDRANT" = 1 ]; then
  QDRANT_IMAGE_NAME="$(toml_get "$COMPANIONS_TOML" "qdrant" "image")"
  QDRANT_IMAGE_TAG="$(toml_get "$COMPANIONS_TOML" "qdrant" "tag")"
  QDRANT_IMAGE_DIGEST="$(toml_get "$COMPANIONS_TOML" "qdrant" "digest")"
  require_pinned "qdrant.image" "$QDRANT_IMAGE_NAME"
  require_pinned "qdrant.tag" "$QDRANT_IMAGE_TAG"
  require_pinned "qdrant.digest" "$QDRANT_IMAGE_DIGEST"
  QDRANT_IMAGE="${QDRANT_IMAGE_NAME}:${QDRANT_IMAGE_TAG}@${QDRANT_IMAGE_DIGEST}"
fi

log "Installing Eris to ${ERIS_HOME} (backend: ${BACKEND})"
log "Hardware floor: GPU with >= ~8GB VRAM and working Vulkan drivers."

# ─── 1. Eris binary ─────────────────────────────────────────────────────────

ERIS_URL="${REL_BASE}/${ERIS_ASSET_X86_64}"
fetch_verified "$ERIS_URL" "${CACHE}/${ERIS_ASSET_X86_64}" "$ERIS_SHA256_X86_64" "eris ${ERIS_TAG}"
tar -xzf "${CACHE}/${ERIS_ASSET_X86_64}" -C "$BIN_DIR" eris \
  || die "could not extract 'eris' from ${ERIS_ASSET_X86_64}. Re-run the installer."
chmod +x "${BIN_DIR}/eris"
log "eris installed: ${BIN_DIR}/eris"

# ─── 2. browser39 CLI (prebuilt upstream — never cargo) ─────────────────────

B39_URL="https://github.com/${BROWSER39_REPO}/releases/download/${BROWSER39_TAG}/${BROWSER39_ASSET_X86_64}"
fetch_verified "$B39_URL" "${CACHE}/${BROWSER39_ASSET_X86_64}" "$BROWSER39_SHA256_X86_64" "browser39 ${BROWSER39_TAG}"
install -m 0755 "${CACHE}/${BROWSER39_ASSET_X86_64}" "${BIN_DIR}/browser39"
if B39_VERSION="$("${BIN_DIR}/browser39" --version 2>/dev/null)"; then
  log "browser39 installed and answering: ${B39_VERSION}"
else
  warn "browser39 installed but '--version' failed. Web tools may not work; re-run the installer or file an issue with this output."
fi

# ─── 3. llama-server (Vulkan) + blessed models ───────────────────────────────

if [ "$BACKEND" != "ollama" ]; then
  LLAMA_ARCHIVE="${CACHE}/llama-${LLAMA_TAG}-bin-ubuntu-vulkan-x64.tar.gz"
  fetch_verified "$LLAMA_VULKAN_URL_X86_64" "$LLAMA_ARCHIVE" "$LLAMA_VULKAN_SHA256_X86_64" "llama-server ${LLAMA_TAG} (Vulkan)"

  EXTRACT_DIR="$(mktemp -d)"
  trap 'rm -rf "$EXTRACT_DIR"' EXIT
  # Official ggml-org Linux packs are .tar.gz (Windows Vulkan is .zip).
  tar -xzf "$LLAMA_ARCHIVE" -C "$EXTRACT_DIR"
  # Archive layouts vary between upstream tags and Eris-built packs; locate
  # llama-server and take everything sitting next to it (shared libs etc).
  SERVER_PATH="$(find "$EXTRACT_DIR" -type f -name llama-server | head -n 1)"
  [ -n "$SERVER_PATH" ] || die "llama-server not found inside the verified archive. The pinned asset layout changed; file an issue."
  cp -f "$(dirname "$SERVER_PATH")"/* "${LLAMA_HOME}/bin/" 2>/dev/null || true
  chmod +x "${LLAMA_HOME}/bin/llama-server"
  log "llama-server (Vulkan) installed: ${LLAMA_HOME}/bin/llama-server"
  log "This binary needs working Vulkan drivers ('vulkaninfo --summary' to check)."
  log "Want CUDA instead? That is a binary swap, not a config key — see the manual."

  fetch_verified "$CHAT_MODEL_URL"  "${MODELS_DIR}/${CHAT_MODEL_FILE}"  "$CHAT_MODEL_SHA256"  "chat model (${CHAT_MODEL_FILE}, ~2.5GB)"
  fetch_verified "$EMBED_MODEL_URL" "${MODELS_DIR}/${EMBED_MODEL_FILE}" "$EMBED_MODEL_SHA256" "embed model (${EMBED_MODEL_FILE})"
fi

# ─── 4. Ollama backend (official installer, pinned; soft-fail after retries) ─

ollama_manual_hint() {
  warn "Ollama could not be installed automatically (pinned install.sh often drifts
when upstream edits https://ollama.com/install.sh).

Install Ollama yourself, then re-run this script or pull models manually:
  1. Open https://ollama.com/download and install for your OS
     (or follow https://ollama.com/install.sh after you trust it)
  2. Confirm the API:  curl -fsS http://localhost:11434/api/version
  3. Pull the pinned tags:
       ollama pull ${CHAT_OLLAMA_TAG}
       ollama pull ${EMBED_OLLAMA_TAG}
Eris llama.cpp companions are already set up if you used --backend both.
You can also re-run with:  --backend llamacpp   to skip Ollama entirely."
}

# Download + sha256-verify with up to 3 attempts. Returns 0 on success, 1 on
# persistent failure (does not abort the whole Eris install — Ollama pin is
# mutable upstream).
fetch_verified_retry() {
  local url="$1" dest="$2" expected="$3" label="$4"
  local attempt
  for attempt in 1 2 3; do
    log "${label}: attempt ${attempt}/3"
    if [ -f "$dest" ]; then
      if [ "$(sha256sum "$dest" | awk '{print $1}')" = "$expected" ]; then
        log "${label}: already present and verified."
        return 0
      fi
      warn "${label}: existing file failed verification; re-downloading."
      rm -f "$dest"
    fi
    if ! curl --proto '=https' --tlsv1.2 -fL --progress-bar -o "${dest}.part" "$url"; then
      warn "${label}: download failed on attempt ${attempt}."
      rm -f "${dest}.part"
      sleep 1
      continue
    fi
    mv "${dest}.part" "$dest"
    local actual
    actual="$(sha256sum "$dest" | awk '{print $1}')"
    if [ "$actual" = "$expected" ]; then
      log "${label}: sha256 verified."
      return 0
    fi
    warn "${label}: checksum mismatch on attempt ${attempt}
  expected: ${expected}
  actual:   ${actual}
Upstream may have updated the file (common for ollama.com/install.sh)."
    rm -f "$dest"
    sleep 1
  done
  return 1
}

if [ "$BACKEND" != "llamacpp" ]; then
  if command -v ollama >/dev/null 2>&1 || curl -fsS --max-time 2 "http://localhost:11434/api/version" >/dev/null 2>&1; then
    log "Ollama already present; skipping install."
  else
    OLLAMA_SCRIPT="${CACHE}/ollama-install.sh"
    if fetch_verified_retry "$OLLAMA_INSTALL_URL" "$OLLAMA_SCRIPT" "$OLLAMA_INSTALL_SHA256" "Ollama official installer"; then
      log "Running the verified official Ollama installer (may prompt for sudo)."
      if ! sh "$OLLAMA_SCRIPT"; then
        warn "Ollama installer script exited non-zero."
        ollama_manual_hint
      fi
    else
      ollama_manual_hint
    fi
  fi
  if command -v ollama >/dev/null 2>&1 && curl -fsS --max-time 3 "http://localhost:11434/api/version" >/dev/null 2>&1; then
    log "Pulling pinned Ollama models: ${CHAT_OLLAMA_TAG}, ${EMBED_OLLAMA_TAG}"
    ollama pull "$CHAT_OLLAMA_TAG"  || warn "'ollama pull ${CHAT_OLLAMA_TAG}' failed — run it manually later."
    ollama pull "$EMBED_OLLAMA_TAG" || warn "'ollama pull ${EMBED_OLLAMA_TAG}' failed — run it manually later."
  elif command -v ollama >/dev/null 2>&1; then
    warn "Ollama CLI is on PATH but API not reachable on localhost:11434. Start it
('ollama serve' or the systemd service), then run:
  ollama pull ${CHAT_OLLAMA_TAG}
  ollama pull ${EMBED_OLLAMA_TAG}"
  else
    warn "Skipping Ollama model pulls (CLI not installed yet). ${CHAT_OLLAMA_TAG} / ${EMBED_OLLAMA_TAG} after you install Ollama."
  fi
fi

# ─── 5. Qdrant via Docker (soft-fail) ────────────────────────────────────────

if [ "$WANT_QDRANT" = 1 ]; then
  cat > "$COMPOSE_FILE" <<EOF
# Qdrant for Eris semantic memory. Docker is for Qdrant ONLY — inference
# (llama-server) is never Dockerized on the supported path.
services:
  qdrant:
    image: ${QDRANT_IMAGE}
    container_name: eris-qdrant
    restart: unless-stopped
    ports:
      - "6333:6333"   # HTTP
      - "6334:6334"   # gRPC (Eris connects here)
    volumes:
      - eris-qdrant-data:/qdrant/storage
volumes:
  eris-qdrant-data:
EOF

  if command -v docker >/dev/null 2>&1 && docker info >/dev/null 2>&1; then
    log "Starting Qdrant (ports 6333/6334, volume eris-qdrant-data)."
    docker compose -f "$COMPOSE_FILE" up -d \
      || warn "docker compose failed. Start Qdrant later with:
  docker compose -f ${COMPOSE_FILE} up -d"
  else
    warn "Docker not available (daemon down, not installed, or no permission).
Eris needs Qdrant for semantic memory. When Docker works, run:
  docker compose -f ${COMPOSE_FILE} up -d
If 'permission denied': add yourself to the docker group and re-login.
To experiment without Qdrant: set require_semantic_brain = false in your
vault's .fcp/config.toml (not recommended for real use)."
  fi
else
  log "Skipping Qdrant start (--no-qdrant). No compose file written."
fi

# ─── 6. env.sh ───────────────────────────────────────────────────────────────

cat > "$ENV_FILE" <<EOF
# Eris environment — 'source ${ENV_FILE}' (add to your shell rc to persist).
export PATH="${BIN_DIR}:\$PATH"
export BROWSER39_BIN="${BIN_DIR}/browser39"
# Notes for vault .fcp/config.toml (ignition defaults to these when present):
#   [llama_cpp]
#   home            = "${LLAMA_HOME}"
#   chat_model_path  = "${MODELS_DIR}/${CHAT_MODEL_FILE:-<chat>.gguf}"
#   embed_model_path = "${MODELS_DIR}/${EMBED_MODEL_FILE:-<embed>.gguf}"
#   n_gpu_layers    = 99
EOF
log "Environment file written: ${ENV_FILE}"

# ─── 7. Next steps ───────────────────────────────────────────────────────────

cat <<EOF

──────────────────────────────────────────────────────────────────────────────
 Eris is installed under ${ERIS_HOME}

 Next steps:
   1. source ${ENV_FILE}
      (add that line to your ~/.bashrc or ~/.zshrc to make it stick)
   2. mkdir -p ~/eris-vault && cd ~/eris-vault     # any Markdown folder works
   3. eris chat                                     # ignition runs first time

 Notes:
   * Shipped llama-server is the VULKAN build. It needs working Vulkan
     drivers and a GPU with >= ~8GB VRAM. CUDA users: swap the binary under
     ${LLAMA_HOME} or retarget llama_cpp.home — see the manual.
   * Default models: Gemma 4 E4B Q4 (chat) + nomic-embed-text (embeddings).
     Other models: drop a GGUF into ${MODELS_DIR} and point your vault
     config at it, or 'ollama pull <tag>' on the Ollama backend.
   * Manual + FAQ: docs/HOW_TO/INSTALL_AND_USER_MANUAL.md in the Eris repo
     (mirrored on the eris-site FAQ).
   * Re-running this installer is safe; it verifies and repairs in place.
──────────────────────────────────────────────────────────────────────────────
EOF

# Releasing Eris (operator cheat sheet)

You do **not** need to be a release engineer. This page is the copy-paste path for:

1. Building a Linux binary  
2. Packing the tarball the installer expects  
3. Getting a **sha256**  
4. (Later) publishing a GitHub Release  

Canonical shipping contract: [`docs/updated_architecture/11_OSS_SHIPPING_ROADMAP.md`](../updated_architecture/11_OSS_SHIPPING_ROADMAP.md) **§5**.

---

## Mental model (30 seconds)

| Piece | Meaning |
|-------|---------|
| `target/…/release/eris` | Raw binary after `cargo build`. **Not** what users download. |
| `eris-x86_64-unknown-linux-gnu.tar.gz` | Packed archive with `eris` at the **root**. This is the Release asset. |
| `sha256sum` of the **`.tar.gz`** | The hash we pin / put in `SHA256SUMS`. Hashing the bare binary alone is the wrong file. |
| `distribution/companions.toml` | Human-edited pin SSOT (companions + Eris asset name/hash until Option B is fully on Release). |
| GitHub Release | Where strangers download from. Release CI should build + hash + upload; local pack is a **dry-run**. |

**Common mistake:**  

```bash
sha256sum eris-x86_64-unknown-linux-gnu.tar.gz
# → No such file or directory
```

That means you built the binary but never **packed** the tarball. Pack first (§ below), then hash.

---

## A. Local dry-run (do this today)

From the **repo root** on Linux x86_64:

### 1. Build

```bash
rustup target add x86_64-unknown-linux-gnu   # once
cargo build --release --target x86_64-unknown-linux-gnu
```

Confirm the binary exists:

```bash
ls -lh target/x86_64-unknown-linux-gnu/release/eris
file target/x86_64-unknown-linux-gnu/release/eris
# expect: ELF 64-bit … x86-64
```

### 2. Pack (installer layout)

The installer runs `tar -xzf … -C "$BIN_DIR" eris`, so the archive must contain a file named **`eris`** at the top level — **not** `release/eris` and **not** a nested folder.

```bash
mkdir -p dist/dry-run
cp -f target/x86_64-unknown-linux-gnu/release/eris dist/dry-run/eris
tar -C dist/dry-run -czf dist/eris-x86_64-unknown-linux-gnu.tar.gz eris
```

Check the archive contents (must list exactly `eris`):

```bash
tar -tzf dist/eris-x86_64-unknown-linux-gnu.tar.gz
# eris
ls -lh dist/eris-x86_64-unknown-linux-gnu.tar.gz
```

### 3. Checksum

```bash
sha256sum dist/eris-x86_64-unknown-linux-gnu.tar.gz
```

Example output shape:

```text
83a2605efe26a6a0f6cdbecb55734544dbea41563412a004b60f981578b725fd  dist/eris-x86_64-unknown-linux-gnu.tar.gz
```

Copy the **64-character hex** (first field only).

### 4. Where to paste (interim, until release CI)

Until Option B + `release.yml` land (roadmap §5):

1. `distribution/companions.toml` → `[eris.assets.linux-x86_64].sha256 = "…"`  
2. `install-linux.sh` → `ERIS_SHA256_X86_64="…"` (same value; mirror until installer fetches pins from the Release)

Also set `[eris].release_tag` / `ERIS_TAG` to the tag you will publish (e.g. `v0.1.1-alpha`).

Mark dry-run pins in a comment: **release CI must regenerate `SHA256SUMS` for the file it actually uploads** — a rebuild can change the hash.

### 5. Optional: write a local `SHA256SUMS` like CI will

```bash
cd dist
sha256sum eris-x86_64-unknown-linux-gnu.tar.gz > SHA256SUMS
cat SHA256SUMS
# later, after download: sha256sum -c SHA256SUMS
```

---

## B. Inspect an existing GitHub Release (`gh`)

Install/auth: [GitHub CLI](https://cli.github.com/) → `gh auth login`.

```bash
# What releases exist?
gh release list --repo janpauldahlke/eris

# What’s on a tag?
gh release view v0.1.1-alpha --repo janpauldahlke/eris

# Download pins + binary from that tag
gh release download v0.1.1-alpha --repo janpauldahlke/eris \
  --pattern 'SHA256SUMS' \
  --pattern 'companions.toml' \
  --pattern 'models.toml' \
  --pattern 'eris-*.tar.gz'

cat SHA256SUMS
sha256sum -c SHA256SUMS --ignore-missing
```

If the Release has **zero assets**, there is nothing to hash yet — finish §A and upload (or wait for release CI).

---

## C. Manual GitHub Release (until automation exists)

When you are ready to publish a dry-run build by hand:

1. Create a git tag matching `companions.toml` / installer (`v0.1.1-alpha` or next).  
2. GitHub → **Releases** → **Draft a new release** (or `gh release create`).  
3. Upload at least:
   - `dist/eris-x86_64-unknown-linux-gnu.tar.gz`
   - `dist/SHA256SUMS` (from §A.5)
   - `distribution/companions.toml`
   - `distribution/models.toml`
   - `install-linux.sh`
4. Publish the release.
5. Confirm with `gh release view <tag>` and `gh release download` (§B).

Example with `gh` (from repo root, after §A):

```bash
TAG=v0.1.1-alpha   # must match pin files

cd dist
sha256sum eris-x86_64-unknown-linux-gnu.tar.gz > SHA256SUMS
cd ..

gh release create "$TAG" \
  --repo janpauldahlke/eris \
  --title "$TAG" \
  --notes "Dry-run / interim Linux x86_64. Prefer release CI once release.yml exists." \
  dist/eris-x86_64-unknown-linux-gnu.tar.gz \
  dist/SHA256SUMS \
  distribution/companions.toml \
  distribution/models.toml \
  install-linux.sh
```

If the tag already exists as a release, use `gh release upload "$TAG" …` instead of `create`.

---

## D. What release CI will do for you later

Roadmap **R1** (`.github/workflows/release.yml`): on tag `v*`, CI builds, packs, writes `SHA256SUMS`, uploads the same asset set. Then you stop hand-packing for real releases; local §A stays useful for smoke tests.

Installer **Option B** (roadmap **R2**): script only hardcodes repo + tag; downloads `companions.toml`, `models.toml`, and `SHA256SUMS` from that Release. No more pasting the Eris hash into two files.

---

## Checklist before you call it “released”

- [ ] `tar -tzf dist/eris-….tar.gz` shows only `eris`  
- [ ] `sha256sum` was run on the **`.tar.gz`**, not only on `target/…/eris`  
- [ ] Hash pasted into pin SSOT (and installer mirror until R2)  
- [ ] Tag name matches pins  
- [ ] Release page lists tarball + `SHA256SUMS` + TOMLs + installer  
- [ ] `gh release download` + `sha256sum -c SHA256SUMS` succeeds on a clean directory  

---

## Related

- Shipping contract §5: [`11_OSS_SHIPPING_ROADMAP.md`](../updated_architecture/11_OSS_SHIPPING_ROADMAP.md)  
- End-user install: [`INSTALL_AND_USER_MANUAL.md`](./INSTALL_AND_USER_MANUAL.md)  
- Build helper script: `scripts/build-release-targets.sh`  

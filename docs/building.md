# Building from Source

## Prerequisites

- **Rust** — stable toolchain (1.75+). Install via [rustup](https://rustup.rs/)
- **Git** — with submodule support
- **Node.js** — 24+ (for MCP servers and Playwright)

### Platform-Specific Dependencies

#### Linux (Ubuntu/Debian)

```bash
sudo apt-get update
sudo apt-get install -y \
  libwebkit2gtk-4.1-dev libjavascriptcoregtk-4.1-dev \
  libsoup-3.0-dev libappindicator3-dev librsvg2-dev \
  build-essential wget file libssl-dev
```

#### macOS

```bash
# Xcode Command Line Tools
xcode-select --install
```

#### Windows

No additional dependencies required. MSVC build tools come with Visual Studio or can be installed standalone.

## Clone & Build

```bash
# Clone with submodules
git clone --recurse-submodules https://github.com/steaven-china/Radiumical.git
cd Radiumical

# If you already cloned without --recurse-submodules:
git submodule update --init

# Build (debug)
cargo build

# Build (release, optimized)
cargo build --release

# Build (release-small, size-optimized)
cargo build --profile release-small
```

Binary locations:
- Debug: `target/debug/radiumical(.exe)`
- Release: `target/release/radiumical(.exe)`
- Release-small: `target/release-small/radiumical(.exe)`

## Run

```bash
# Run directly
cargo run --bin radiumical

# Run with arguments
cargo run --bin radiumical -- -p openai -m gpt-4o

# Install locally
cargo install --path radiumical-tui
```

## Test

```bash
# Run all tests
cargo test --workspace --all-features

# Run with output
cargo test --workspace --all-features -- --nocapture

# Run specific test
cargo test -p radiumical-core test_name
```

## Lint & Format

```bash
# Check formatting
cargo fmt --all -- --check

# Apply formatting
cargo fmt --all

# Run clippy
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

## Release Profiles

| Profile | Opt Level | LTO | Use Case |
|---------|-----------|-----|----------|
| `dev` | 0 | off | Development (fast compile) |
| `release` | 3 | fat | Maximum performance |
| `release-small` | z | fat | Distribution (small binary) |

### release-small

The `release-small` profile trades some performance for binary size:

```toml
[profile.release-small]
inherits = "release"
opt-level = "z"
codegen-units = 1
lto = "fat"
panic = "abort"
strip = true
```

This produces the smallest binary, suitable for distribution.

## Workspace Crates

| Crate | Type | Description |
|-------|------|-------------|
| `radiumical-core` | lib | Core library (agent, providers, tools, persistence) |
| `radiumical-tui` | bin | TUI frontend (the main `radiumical` binary) |
| `radiumical-tauri` | lib | Tauri desktop app (experimental) |
| `radiumical-rpc` | bin | RPC server for external integrations |
| `csv-to-jsonl` | bin | Provider registry conversion tool |
| `memory-bench` | bin | Memory and compression benchmark |

## Submodules

The `providers-record` submodule contains the embedded provider registry:

```bash
# Update submodule
git submodule update --remote providers-record
```

## CI

GitHub Actions runs on every push/PR to `main`:

1. **Lint** — `cargo fmt --check` + `cargo clippy -D warnings`
2. **Test** — `cargo test --workspace --all-features`
3. **Build** — cross-platform matrix (Linux, macOS, Windows)
4. **Bench** — memory-bench tool
5. **Smoke** — `radiumical --help` + `radiumical --version`

## Release

Releases are tag-triggered:

```bash
# Pre-release
git tag pre-v0.1.0
git push origin pre-v0.1.0

# Full release
git tag v0.1.0
git push origin v0.1.0
```

The release workflow builds `release-small` binaries for all platforms and creates a GitHub Release with the binaries attached. Pre-release tags (`pre-v*`) are automatically marked as prerelease.

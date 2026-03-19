# Contributing to SomaOS

SomaOS is an early-stage, school nights project. The codebase has sharp edges and a lot left to build. If you're here, you're not a user — you're a builder. Welcome.

---

## What We're Looking For

- **Contributors** — Rust systems engineers, compositor hackers, people who want to work on AI-native UX
- **Maintainers** — people who want to own a subsystem long-term (compositor, agent runtime, capability modules, AgentAPI spec)
- **Validators** — researchers and engineers who want to stress-test the ideas, especially around AgentAPI design

---

## Before You Start

Read [CLAUDE.md](CLAUDE.md) — it's the living architecture guide for the project. It covers the thesis, the stack, key files, gotchas, and open design questions. It will save you a lot of time.

---

## Dev Environment

### Prerequisites

- Rust toolchain (`rustup` recommended)
- For bare-metal targets: `x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl` targets
- For macOS dev (winit backend): standard Rust + Cargo is enough
- Ollama running locally at `http://localhost:11434` with `qwen2.5-coder:7b` pulled (for agent features)

### Build

```bash
# macOS dev build (winit backend)
cargo build -p soma-agent
cargo build -p soma-compositor

# Cross-compile check (what CI runs)
cargo check -p soma-agent --target x86_64-unknown-linux-musl
cargo check -p soma-compositor --features drm-backend --target x86_64-unknown-linux-musl
```

See [docs/BUILD_x86_64.md](docs/BUILD_x86_64.md) and [docs/BUILD_ARM64.md](docs/BUILD_ARM64.md) for full build instructions including Buildroot OS images.

### Run Tests

```bash
cargo run -p soma-cli -- --test
```

61 scenarios covering all capability modules. All should pass.

---

## Project Structure

```
soma-common/        Shared IPC types
soma-agent/         Agent daemon (LLM providers, capability registry)
soma-compositor/    Compositor (DRM/KMS + tiny-skia, floating WM)
soma-cli/           CLI test client
buildroot/          OS image build system
docs/               Architecture docs, build guides
```

---

## How to Contribute

1. **Check open issues** — look for `good first issue` or `help wanted` labels
2. **Open an issue first** for anything non-trivial — alignment before code saves everyone time
3. **Fork and branch** — branch off `main`, keep branches focused
4. **Run the pre-flight checks** before submitting a PR:

```bash
cargo build -p soma-agent
cargo build -p soma-compositor
cargo check -p soma-agent --target x86_64-unknown-linux-musl
cargo check -p soma-compositor --features drm-backend --target x86_64-unknown-linux-musl
cargo run -p soma-cli -- --test
```

5. **Update docs** — if your change affects architecture, capabilities, or build steps, update the relevant `.md` files
6. **Submit a PR** — keep the description focused on *why*, not just *what*

---

## Key Design Invariants

These are non-negotiable — don't send a PR that breaks them:

- **HITL gate is sacred** — the `meta.propose` flow always requires human approval. Never bypass it.
- **`desktop_agent` command pattern** — capabilities return an `ipc_message` key; the IPC handler forwards it. Don't route around this.
- **Feature gates** — anything touching display/input must be gated: `winit-backend` for dev, `drm-backend` for production. Both must compile.
- **`main.rs` stays under 800 lines** — it was already split once. Keep it that way.

---

## Open Design Questions

The most valuable contributions right now are opinions on these:

1. What should `AgentAPI::describe_state` return for a spreadsheet? This contract shapes all future apps.
2. How do human edits and agent writes coexist on the same data model? Cell-level locking? Last-write-wins? OT/CRDT?
3. What does an agent session scope look like? JSON config? Capability whitelist?
4. Should semantic FS metadata live as sidecars (`.soma-meta`) or a central index (`~/.soma/index.db`)?

Open an issue or start a Discussion if you have thoughts.

---

## License

MIT. See [LICENSE](LICENSE).

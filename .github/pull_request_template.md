**What does this PR do**

**Why**

**Checklist**
- [ ] `cargo build -p soma-agent` passes
- [ ] `cargo build -p soma-compositor` passes
- [ ] `cargo check -p soma-agent --target x86_64-unknown-linux-musl` passes
- [ ] `cargo check -p soma-compositor --features drm-backend --target x86_64-unknown-linux-musl` passes
- [ ] `cargo run -p soma-cli -- --test` — all 61 scenarios pass
- [ ] Relevant `.md` files updated if architecture/capabilities/build steps changed
- [ ] Design invariants respected (HITL gate, command pattern, feature gates, main.rs < 800 lines)

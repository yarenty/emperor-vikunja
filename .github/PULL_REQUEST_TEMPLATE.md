## What changes

<!-- What a user of the server notices. -->

## Checklist

- [ ] Every commit is signed off (`git commit -s`, [DCO](https://developercertificate.org/))
- [ ] `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, `cargo deny check` pass
- [ ] Live test run when a request or response shape changed (`cargo test --test live_vikunja -- --ignored`)
- [ ] README, manifest.yaml, AGENTS.md and CHANGELOG updated where behaviour changed
- [ ] No token, password or private URL in code, tests, logs or this description

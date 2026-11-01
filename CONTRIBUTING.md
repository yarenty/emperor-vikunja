# Contributing to emperor-vikunja

Thanks for helping. Small, focused pull requests are the easiest to review; open an issue first
for a new tool or anything that changes how credentials are handled (see
[AGENTS.md](./AGENTS.md) and the emperor-mcp
[Emperor Profile](https://github.com/yarenty/emperor-mcp/blob/main/PROFILE.md)).

## Before you open a pull request

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo deny check
cargo test --test live_vikunja -- --ignored   # needs Docker; when you changed what goes over the wire
```

Keep the docs in step with the code: `README.md`, `manifest.yaml`, `AGENTS.md` and a
`CHANGELOG.md` entry under `[Unreleased]` for anything a user would notice. Every public item
is documented (`#![warn(missing_docs)]`).

## Developer Certificate of Origin

This project uses the [Developer Certificate of Origin](https://developercertificate.org/)
(DCO) instead of a contributor licence agreement. By adding a sign-off line to your commits
you certify that you wrote the change or otherwise have the right to submit it under the
project licence.

Sign off every commit:

```bash
git commit -s
```

which adds a line such as

```
Signed-off-by: Your Name <you@example.com>
```

Use a real name and a reachable email address.

## Licence

emperor-vikunja is dual licensed under MIT OR Apache-2.0 (see [LICENSE-MIT](./LICENSE-MIT) and
[LICENSE-APACHE](./LICENSE-APACHE)). Unless you explicitly state otherwise, any contribution
you submit is licensed the same way, without any additional terms or conditions.

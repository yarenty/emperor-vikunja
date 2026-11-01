# emperor-vikunja — AI agent notes

**Crate**: `emperor-vikunja` · **Version**: 0.1.0 · **Licence**: MIT OR Apache-2.0 · **Rust**: 1.88 (edition 2024; `rust-version` matches the highest MSRV in the dependency graph, rmcp 3.8)

## Scope

A **standalone MCP server for a Vikunja instance**, built on [`emperor-mcp`](https://github.com/yarenty/emperor-mcp) 0.2 (rmcp 3.8 underneath) and held to the Emperor Profile P1. Six tools, a thin hand-written REST client, a `doctor` subcommand. Nothing else: no caching, no background work, no state between calls.

Layout (one package, lib + bin):

- `src/client.rs` — `Vikunja` (base URL + `reqwest` pool, no credentials), `Session` (one borrowed `Token` for one tool call), the API shapes, `TaskQuery` (filter expression + the same filter applied locally), `sort_tasks`. Only the endpoints the tools use.
- `src/server.rs` — `VikunjaServer`: the rmcp `#[tool_router]` with the six tools, token lookup, project lookup by id or title, output framing.
- `src/text.rs` — Markdown → HTML (`pulldown-cmark`, raw HTML escaped) and HTML → text (`html2text`).
- `src/dates.rs` — date arguments (`YYYY-MM-DD` or RFC 3339) and Vikunja's zero time (`0001-01-01T00:00:00Z` = unset).
- `src/doctor.rs` — the `doctor` checks, built on `emperor_mcp::doctor`.
- `src/main.rs` — clap CLI: `serve [--stdio]`, `doctor`.
- `tests/common/` — the fake Vikunja (axum, canned v2.x-shaped JSON, every request recorded, real pagination) and MCP-over-HTTP helpers.
- `tests/client.rs`, `tests/mcp_http.rs`, `tests/stdio.rs` — client against the fake; the server end to end over stateless HTTP through `emperor_mcp::serve`; the real binary over stdio.
- `tests/live_vikunja.rs` — `#[ignore]`: every tool against a `vikunja/vikunja` container (CI job `live-vikunja`).

## Before you change code

1. Read `src/server.rs` (tool flow) and `src/client.rs` (what goes over the wire).
2. Run `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps` and `cargo deny check`.
3. If you touched a request or response shape, run the live test too: `cargo test --test live_vikunja -- --ignored --nocapture` (needs Docker). The fake only proves what we send; the container proves Vikunja accepts it.

## Conventions

- **Six tools, three to eight ever.** A seventh tool needs a reason; past eight the server is split by noun (MCP_REQUIREMENTS §11). `doctor` counts the real router, and `server::tests::exactly_six_tools` pins the names.
- **Credentials are forwarded, never stored (E6).** HTTP mode reads the token from the forwarded `Authorization` header (`ForwardConfig::authorization_only()`, the `ServeOptions::new` default) on every call; stdio mode reads `VIKUNJA_TOKEN` once at start-up. Both go through `VikunjaServer::token` into `Vikunja::session`. HTTP mode never falls back to the env token: a shared server must not answer one user's call with another user's token. `Token`'s `Debug` is redacted; tokens never appear in logs, errors, URLs or files.
- **No shortcuts (E7).** No default token, no test token, no "skip auth" branch. Tests mint a random token per run for the fake; the live test registers a user with a random password.
- **Frame every result (E8).** `get_task` returns user-written text, so it is `FrameKind::TrustedReference`; listings and write confirmations are `FrameKind::Computed`. Always through `emperor_mcp::structured_framed` (framed text for the model, raw JSON as `structuredContent`). The breakout test is `server::tests::task_text_cannot_break_out_of_the_frame`.
- **Errors are tool results, not protocol errors.** Missing token, bad arguments and Vikunja refusals come back as `isError: true` with a sentence the model can act on; nothing panics on upstream failure. Every upstream call has a timeout (`client::REQUEST_TIMEOUT`).
- **Updates send the whole task.** Vikunja's `POST /tasks/{id}` replaces every field, so `update_task` fetches the task as JSON, changes only the requested keys and posts it back. Labels change through the label endpoints, never through the task body.
- **Filter twice.** `TaskQuery::filter_expression` asks Vikunja to filter; `TaskQuery::matches` applies the same rules locally. Keep the two in step.
- **Logs to stderr** (`emperor_vikunja::init_tracing`), so stdout stays clean for stdio (E9).

### Why `html2text` for HTML → text

Vikunja descriptions come from a rich-text editor: nested lists, tables, code blocks, entities. `html2text` parses with `html5ever` (the Servo HTML5 parser), so malformed or unusual markup still renders sensibly, and it is actively maintained. A hand-written converter would need its own tokenizer and entity table to get the same result. The cost is compile time and the html5ever crates; there is no `unsafe` of ours and no network or file access in it.

## Documentation closure (mandatory)

After any behaviour change update [`README.md`](./README.md) (tools table, token permissions, connect snippets), [`manifest.yaml`](./manifest.yaml) (tool list) and add a [`CHANGELOG.md`](./CHANGELOG.md) entry under `[Unreleased]`. **Shipping code without updating docs is incomplete work.**

## Release

Tag `vX.Y.Z` on `main`: `.github/workflows/release.yml` builds Linux x86_64, macOS arm64 and x86_64 and Windows x86_64 binaries and attaches them to the GitHub release. Then `cargo publish`.

## Related docs

- [`README.md`](./README.md) — install, token, run, connect, tools, doctor
- [`CHANGELOG.md`](./CHANGELOG.md) — what each release changed
- [`CONTRIBUTING.md`](./CONTRIBUTING.md) — checks and DCO sign-off
- emperor-mcp [`PROFILE.md`](https://github.com/yarenty/emperor-mcp/blob/main/PROFILE.md) and [`MCP_REQUIREMENTS.md`](https://github.com/yarenty/emperor-mcp/blob/main/MCP_REQUIREMENTS.md) — the rules this server follows

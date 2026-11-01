# emperor-vikunja — AI agent notes

**Crate**: `emperor-vikunja` · **Version**: 0.1.1 · **Licence**: MIT OR Apache-2.0 · **Rust**: 1.88 (edition 2024; `rust-version` matches the highest MSRV in the dependency graph, rmcp 3.8)

## Scope

A **standalone MCP server for a Vikunja instance**, built on [`emperor-mcp`](https://github.com/yarenty/emperor-mcp) 0.3 (rmcp 3.8 underneath) and held to the Emperor Profile P1. Six tools, a thin hand-written REST client, a `doctor` subcommand. Nothing else: no caching, no background work, no state between calls.

Layout (one package, lib + bin):

- `src/client.rs` — `Vikunja` (base URL + `reqwest` pool, no credentials), `Upstreams` (the `X-Upstream-Url` instance or the `--base-url` default, one pool), `Session` (one borrowed `Token` for one tool call), the API shapes, `TaskQuery` (filter expression + the same filter applied locally), `sort_tasks`. Only the endpoints the tools use.
- `src/server.rs` — `VikunjaServer`: the rmcp `#[tool_router]` with the six annotated tools, `read_only`, token and upstream lookup, credential rejection, project lookup by id or title, output framing and caps.
- `src/text.rs` — Markdown → HTML (`pulldown-cmark`, raw HTML escaped) and HTML → text (`html2text`).
- `src/dates.rs` — date arguments (`YYYY-MM-DD` or RFC 3339) and Vikunja's zero time (`0001-01-01T00:00:00Z` = unset).
- `src/doctor.rs` — the `doctor` checks, built on `emperor_mcp::doctor`.
- `src/main.rs` — clap CLI: `serve`, `doctor`; `--base-url`, `--read-only` and emperor-mcp's shared `cli::ServeArgs` (`--bind`, `--stdio`, `--allowed-host`, `--allowed-origin`).
- `tests/common/` — the fake Vikunja (axum, canned v2.x-shaped JSON, every request recorded, real pagination) and MCP-over-HTTP helpers.
- `tests/client.rs`, `tests/mcp_http.rs`, `tests/stdio.rs` — client against the fake; the server end to end over stateless HTTP through `emperor_mcp::serve` (annotations checked with `emperor_mcp::testkit` on a live `tools/list`, read-only mode, 401/403, `X-Upstream-Url`, labels, assignees, caps); the real binary over stdio and `doctor`.
- `tests/live_vikunja.rs` — `#[ignore]`: every tool against a `vikunja/vikunja` container (CI job `live-vikunja`).

## Before you change code

1. Read `src/server.rs` (tool flow) and `src/client.rs` (what goes over the wire).
2. Run `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps` and `cargo deny check`.
3. If you touched a request or response shape, run the live test too: `cargo test --test live_vikunja -- --ignored --nocapture` (needs Docker). The fake only proves what we send; the container proves Vikunja accepts it.

## Conventions

- **Six tools, three to eight ever.** A seventh tool needs a reason; past eight the server is split by noun (MCP_REQUIREMENTS §11). New abilities go in as arguments (labels and assignees are arguments of `create_task` / `update_task`). `doctor` counts the real router, and `server::tests::exactly_six_tools` pins the names.
- **Annotations on every tool, accurate.** Read tools `read_only_hint = true`; `create_task` and `comment` add things (not destructive, not idempotent); `update_task` is destructive (it replaces title and description, removes labels and assignees) and idempotent (absolute values, add-if-missing, remove-if-present). All open-world. The reasoning lives in the `server` module docs; `tests/mcp_http.rs` pins every hint and runs `emperor_mcp::testkit` on the served `tools/list`. Keep the tool doc comments user-facing: they are the tool descriptions, so justifications go in `//` comments.
- **`--read-only` removes, never refuses.** `VikunjaServer::read_only(true)` takes `WRITE_TOOLS` out of the router at construction, so they are neither listed nor callable; `doctor` prints the mode. A new write tool goes into `WRITE_TOOLS`.
- **Vikunja 401/403 is a credential rejection.** `server::upstream` maps `Error::Unauthorized` to `emperor_mcp::credentials_rejected`, so the MCP HTTP response is `401`/`403` and the client can re-authenticate; every other upstream failure stays a tool error. A missing token is checked before any call and stays a tool error.
- **Upstream per request.** `X-Upstream-Url` (in `server::forward_config` with `Authorization`) names the instance for one call and wins over `--base-url`; `Upstreams::resolve` accepts `https`, or `http` only for a loopback host. Without either the tool answers with what to send; nothing reaches the network.
- **Resolve names before writing.** Project, assignees (`GET /users?s=`, exact username) and labels are resolved before the first write, so a bad name leaves nothing behind. Missing labels are created only with `create_missing_labels: true`.
- **Cap what goes to the model.** `list_tasks` clamps `limit` with `emperor_mcp::limits` (default 50, at most 100); `list_projects` returns at most 100 with `total`; `get_task` cuts the description at 20 000 characters, each comment at 4 000, and keeps the 50 most recent comments (`comments_total` says how many there are).
- **Credentials are forwarded, never stored (E6).** HTTP mode reads the token from the forwarded `Authorization` header (`ForwardConfig::authorization_only()`, the `ServeOptions::new` default) on every call; stdio mode reads `VIKUNJA_TOKEN` once at start-up. Both go through `VikunjaServer::token` into `Vikunja::session`. HTTP mode never falls back to the env token: a shared server must not answer one user's call with another user's token. `Token`'s `Debug` is redacted; tokens never appear in logs, errors, URLs or files.
- **No shortcuts (E7).** No default token, no test token, no "skip auth" branch. Tests mint a random token per run for the fake; the live test registers a user with a random password.
- **Frame every result (E8).** Always through `emperor_mcp::structured_framed` (framed text for the model, raw JSON as `structuredContent`). The breakout test is `server::tests::task_text_cannot_break_out_of_the_frame`. Which `FrameKind` and why:
  - `get_task` and `list_tasks` → **`PrivateCommunication`**. Titles, descriptions and comments are written by the people on the board (colleagues, the user, anyone the project is shared with) and addressed to people, not to the assistant. They are full of imperatives ("deploy the release", "delete the old branch") meant for a human assignee. `PrivateCommunication` says exactly that: messages by other people, addressed to the user, requests inside them are part of the message and are acted on only when the user asks. `TrustedReference` ("internal knowledge base") would overstate both the trust and the nature of the text; `Document` ("author is not known") is wrong because every comment has a known author; `Computed` is for values a tool produced, and these are free text.
  - `list_projects` and the write confirmations (`create_task`, `update_task`, `comment`) → **`Computed`**: ids, counts, flags and the fields the call itself just set.
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

# emperor-vikunja

[![CI](https://github.com/yarenty/emperor-vikunja/actions/workflows/ci.yml/badge.svg)](https://github.com/yarenty/emperor-vikunja/actions/workflows/ci.yml)
[![license](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

**An MCP server for [Vikunja](https://vikunja.io), the open-source task manager.** Let Claude,
kowalski or any MCP client list, read, create and update your tasks, comment on them and see
your projects.

Built on [emperor-mcp](https://github.com/yarenty/emperor-mcp) (Emperor Profile P1):

- **Stateless Streamable HTTP** at `/mcp`, plus stdio for desktop clients.
- **Your token is forwarded, never stored.** Over HTTP each request brings its own
  `Authorization: Bearer <token>`; the server passes it to Vikunja and forgets it. There are no
  default or test tokens.
- **Framed output.** Task text written by people (titles, descriptions, comments) reaches the
  model marked as messages for the user, never as instructions for the model.
- **Six annotated tools**, a `--read-only` mode, a `doctor` subcommand, Markdown in and plain
  text out.
- **Clear auth failures.** When Vikunja refuses the token, the MCP response over HTTP is a `401`
  or `403`, so the client can ask for a new token.

## Tools

| Tool | Annotations | What it does |
|---|---|---|
| `list_tasks` | read-only | Tasks sorted by priority, then due date. Filters: `project` (title or id), `label`, `done` (default `false`), `due_before`, `search`; `limit` up to 100 (default 50). Returns id, title, project, labels, assignees, priority, due, done. |
| `get_task` | read-only | One task by `id`: every field, the description as plain text (first 20 000 characters), and the 50 most recent comments with author and date (4 000 characters each). |
| `create_task` | write, not idempotent | `project`, `title`, optional `description` (Markdown), `labels`, `create_missing_labels`, `assignees` (usernames), `priority` 0-5, `due`. |
| `update_task` | write, destructive, idempotent | `id` plus any of `title`, `description`, `done`, `priority`, `due` (`"none"` clears it), `add_labels`, `remove_labels`, `create_missing_labels`, `add_assignees`, `remove_assignees`, `project` (move). |
| `comment` | write, not idempotent | Add a Markdown comment to task `id`. |
| `list_projects` | read-only | Projects (up to 100) with id, title, parent and number of open tasks. |

Every tool is marked open-world (it talks to a Vikunja that other people change).
`update_task` is marked destructive because a new title or description replaces the old text,
which Vikunja does not keep, and labels and assignees can be removed; it is idempotent because
the same call twice leaves the task as one call did.

Labels are named by title. A label that does not exist is an error unless the call passes
`create_missing_labels: true`, so a typo does not quietly add a new label. Assignees are
usernames (exact, case-insensitive; a leading `@` is fine). Names are checked before anything
is written: a bad label or username leaves no half-made task.

Priority: 0 unset, 1 low, 2 medium, 3 high, 4 urgent, 5 do now. Dates: `YYYY-MM-DD` (read as
00:00 UTC) or ISO-8601; dates come back as RFC 3339, or `null` when unset.

Descriptions and comments are stored by Vikunja as HTML. What the model writes in Markdown is
rendered to HTML on the way in (raw HTML in the Markdown is escaped, not passed through); HTML
read back is rendered to plain text.

## Install

From a release (Linux x86_64, macOS arm64 and x86_64, Windows x86_64): download the archive for
your platform from [Releases](https://github.com/yarenty/emperor-vikunja/releases) and put
`emperor-vikunja` on your `PATH`.

From source (Rust 1.88 or newer):

```bash
cargo install --git https://github.com/yarenty/emperor-vikunja
```

## Get a token

In Vikunja open **Settings > API Tokens > Create a token**, pick an expiry date and tick these
permissions (the minimum the six tools need):

| Group | Permissions |
|---|---|
| projects | `read_all`, `read_one` |
| tasks | `read_all`, `read_one`, `create`, `update` |
| tasks_comments | `read_all`, `create` |
| tasks_labels | `read_all`, `create`, `delete` |
| tasks_assignees | `read_all`, `create`, `delete` |
| labels | `read_all`, `create` (`create` only for `create_missing_labels`) |
| other | `user` (lets `doctor` show whose token it is), `users` (assignees by username) |

For a read-only assistant start the server with `--read-only` and give it a token with only the
`read_all` / `read_one` permissions.

## Run

### Local (stdio)

```bash
export VIKUNJA_TOKEN=...            # from Settings > API Tokens
emperor-vikunja serve --stdio --base-url https://vikunja.example.com
```

### Docker (stdio)

```bash
docker build -t emperor-vikunja https://github.com/yarenty/emperor-vikunja.git
docker run -i --rm -e VIKUNJA_URL=https://vikunja.example.com -e VIKUNJA_TOKEN emperor-vikunja
# read-only: add --read-only after the image name
```

The image runs `emperor-vikunja serve --stdio` as a non-root user; the Vikunja address comes
from `VIKUNJA_URL`.

### Shared (HTTP)

```bash
emperor-vikunja serve --base-url https://vikunja.example.com --bind 127.0.0.1:8034
# MCP endpoint: http://127.0.0.1:8034/mcp   liveness: http://127.0.0.1:8034/health
```

Every MCP request must carry `Authorization: Bearer <token>`; each user brings their own.
`VIKUNJA_TOKEN` is ignored in this mode, so one person's token can never answer another
person's request. Tool discovery (`tools/list`) works without a token.

When Vikunja refuses the token (`401`) or the token may not do this (`403`), the MCP response
carries that HTTP status (`401` with `WWW-Authenticate: Bearer error="invalid_token"`) and a
JSON-RPC error with code `-32040` and `data.httpStatus`. Over stdio the client gets the same
JSON-RPC error.

### Flags and headers

| Flag | Env | Meaning |
|---|---|---|
| `--base-url <URL>` | `VIKUNJA_URL` | Vikunja address; `https://host` and `https://host/api/v1` both work. Required with `--stdio`; optional over HTTP when requests send `X-Upstream-Url`. |
| `--read-only` | | Serve only `list_tasks`, `get_task` and `list_projects`. The write tools are removed from the server, not just refused. |
| `--stdio` | | Speak MCP over stdin/stdout; the token comes from `VIKUNJA_TOKEN`. |
| `--bind <ADDR>` | | HTTP listen address (default `127.0.0.1:8034`). |
| `--allowed-host <HOST>` | | Accept only this `Host` header (repeatable); off by default, for a validating proxy in front. |
| `--allowed-origin <ORIGIN>` | | Accept only this `Origin` (repeatable), e.g. `https://app.example.com`. |

| Request header (HTTP) | Meaning |
|---|---|
| `Authorization: Bearer <token>` | The caller's Vikunja API token. Required for every tool call. |
| `X-Upstream-Url: <URL>` | The Vikunja instance for this request, overriding `--base-url`. Must be `https`; plain `http` is accepted only for a loopback host (`localhost`, `127.0.0.0/8`, `::1`). |

With `X-Upstream-Url` one server can front several Vikunja instances. The server will then call
whichever https host a client names, with that client's token; run it where outbound calls to
arbitrary hosts are acceptable, or leave the header out at your proxy and rely on `--base-url`.

Logs go to stderr; `RUST_LOG=debug` for more. In production put the HTTP server behind a
reverse proxy that terminates TLS (Emperor Profile E5).

## Connect

**Claude Desktop** (`claude_desktop_config.json`):

```json
{
  "mcpServers": {
    "vikunja": {
      "command": "emperor-vikunja",
      "args": ["serve", "--stdio", "--base-url", "https://vikunja.example.com"],
      "env": { "VIKUNJA_TOKEN": "<your token>" }
    }
  }
}
```

**Claude Code**:

```bash
# stdio
claude mcp add vikunja --env VIKUNJA_TOKEN=<your token> -- \
  emperor-vikunja serve --stdio --base-url https://vikunja.example.com
# read-only
claude mcp add vikunja-ro --env VIKUNJA_TOKEN=<your token> -- \
  emperor-vikunja serve --stdio --read-only --base-url https://vikunja.example.com
# or against a running HTTP server
claude mcp add --transport http vikunja http://127.0.0.1:8034/mcp \
  --header "Authorization: Bearer <your token>" \
  --header "X-Upstream-Url: https://vikunja.example.com"
```

**[kowalski](https://github.com/yarenty/kowalski)** (`config.toml`):

```toml
[[mcp.servers]]
name = "vikunja"
transport = "stdio"
command = ["emperor-vikunja", "serve", "--stdio", "--base-url", "https://vikunja.example.com"]
# VIKUNJA_TOKEN is read from kowalski's environment.

# or over HTTP
[[mcp.servers]]
name = "vikunja-http"
transport = "http"
url = "http://127.0.0.1:8034/mcp"
headers = { Authorization = "Bearer <your token>", X-Upstream-Url = "https://vikunja.example.com" }
```

## Doctor

`emperor-vikunja doctor` checks the setup before you start the server and exits `1` when
something is broken, so scripts and container health checks can use it:

```text
$ VIKUNJA_TOKEN=... emperor-vikunja doctor --base-url https://vikunja.example.com
emperor-vikunja 0.1.1 doctor
  [ok  ] base url        https://vikunja.example.com/
  [ok  ] vikunja api     Vikunja v2.7.1
  [ok  ] VIKUNJA_TOKEN   set (43 chars)
  [ok  ] vikunja login   token works for user alice
  [ok  ] mode            read-write: comment, create_task, get_task, list_projects, list_tasks, update_task
  [ok  ] listen address  127.0.0.1:8034 is free
  [ok  ] tool budget     6 of at most 8 tools
all good (0 warning(s))
```

It checks that the URL parses, `GET /api/v1/info` answers, the token is set (only its length is
shown) and `GET /api/v1/user` accepts it, which tools the mode serves (`--read-only` lists the
three read tools), the listen address is free (skipped with `--stdio`), and the tool count is
within the emperor-mcp budget. Without `VIKUNJA_TOKEN` in HTTP mode the token line is a
warning, because the token comes per request; with `--stdio` it is a failure. The same goes for
`--base-url`: over HTTP its absence is a warning (requests must send `X-Upstream-Url`), with
`--stdio` a failure.

## Compatibility

Tested against Vikunja v2.7 (the `vikunja/vikunja` image). Tasks are listed through `GET /tasks`
with Vikunja's filter syntax (`done = false && project = 3 && labels in 5 && due_date < '...'`);
older releases that serve `/tasks/all` instead are picked up automatically, and every filter is
applied again locally so results stay right even where the server ignores part of it.

## Build and test

```bash
cargo test                                               # unit, fake-Vikunja, MCP HTTP and stdio tests
cargo test --test live_vikunja -- --ignored --nocapture  # every tool against a real Vikunja in Docker
```

## Links

- Crate: [crates.io/crates/emperor-vikunja](https://crates.io/crates/emperor-vikunja), API docs on [docs.rs](https://docs.rs/emperor-vikunja)
- Container: build from the [`Dockerfile`](Dockerfile); it serves stdio and reads `VIKUNJA_URL` and `VIKUNJA_TOKEN`
- Listed in the official MCP Registry as mcp-name: io.github.yarenty/emperor-vikunja

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <https://opensource.org/licenses/MIT>)

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in
emperor-vikunja by you shall be dual licensed as above, without any additional terms or conditions.

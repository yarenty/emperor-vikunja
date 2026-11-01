# emperor-vikunja next to other task-manager MCP servers

How emperor-vikunja 0.1.2 compares with the public MCP servers for
[Vikunja](https://vikunja.io), with Todoist's and Linear's own servers for reference. For the
shared background (the 2026-07-28 spec revision, annotations, the "lethal trifecta") and what
every emperor server gets from the framework, see
[the emperor-mcp comparison](https://github.com/yarenty/emperor-mcp/blob/main/docs/COMPARISON.md).

Figures for other projects were read on **2026-11-01** from the GitHub API (stars, last
release) and from each project's README and source (tool counts). A **(?)** marks a figure that
could not be confirmed; "not re-counted" means an earlier count was kept.

## Why this domain matters

A task board is less dangerous than a mailbox, but a token that can write can also rename,
re-describe and close everything on the board, and task text is written by other people. The
questions are the same: can the assistant be limited to reading, can it delete, and does the
server keep your token.

## The table

| Server | Stars | Last release | Language | Tools | Gating | Activity |
|---|---|---|---|---|---|---|
| **emperor-vikunja** | new | 0.1.2, 2026-11-01 | Rust | 6 (3 with `--read-only`) | `--read-only` removes the write tools; no delete tool | Live test against Vikunja 2.7.1; API v1 paths |
| [democratize-technology/vikunja-mcp](https://github.com/democratize-technology/vikunja-mcp) | 114 | v0.2.0, 2025-08-16 | TypeScript | 13 multiplexed (each with many sub-commands) | None; no annotations; bulk delete without confirmation | Last commit 2026-02-15, 40 open pull requests |
| [aimbitgmbh/vikunja-mcp](https://github.com/aimbitgmbh/vikunja-mcp) | 12 | v1.1.0, 2026-10-30 | TypeScript | 52 | Deletes off by default, one switch per resource | Vikunja 2.6+ on API v2 only |
| [Doist/todoist-mcp](https://github.com/Doist/todoist-mcp) (Todoist) | 562 | v13.8.2, 2026-10-31 | TypeScript | about 45, not re-counted | Annotations, with a test that checks them | Very active, by the vendor |
| [Linear MCP](https://linear.app/docs/mcp) (Linear) | closed source | — | — | about 20 (?) | A separate read-only endpoint and read-only keys | Hosted, by the vendor |

**What "multiplexed tools" means:** one tool such as `vikunja_tasks` takes a `command` argument
(`list`, `create`, `delete`, …). The tool count looks small, but 50+ operations hide behind a few
names. The client can no longer tell a read from a delete, and the model has to learn a second
language inside the tool. emperor-vikunja avoids this on purpose: one tool, one verb.

## emperor-vikunja: pros

- **Small and clear**: 6 tools, each a verb, and no delete tool at all.
- **`--read-only`** removes the three write tools from the server (the pattern Linear uses), and
  the README lists the matching read-only token permissions.
- **Labels and assignees by name.** An unknown label is an error unless the call asks to create
  it, so a typo does not add a new label; every name is checked before anything is written, so a
  bad name leaves no half-made task.
- **Markdown in, plain text out.** Vikunja stores HTML; descriptions and comments written in
  Markdown are rendered on the way in (raw HTML escaped) and read back as text.
- **The token is forwarded, never stored.** Over HTTP each request brings its own
  `Authorization: Bearer` token; `VIKUNJA_TOKEN` is ignored there, so one person's token never
  answers another person's request. When Vikunja refuses it, the MCP response is HTTP 401 or 403.
- **One server, several instances**: `X-Upstream-Url` names the Vikunja for a request (https, or
  http on loopback only).
- **Task text is framed** as messages written by people for the user, not instructions for the
  model.
- **Annotations on every tool**, checked by a test; live-tested against a real Vikunja; stdio and
  HTTP; one binary with provenance.

## emperor-vikunja: cons

- **Covers the daily work, not the whole API**: no project creation or editing, no relations,
  buckets (kanban), attachments, reminders or saved filters.
- **API v1 paths.** Vikunja 2.7 still serves them, but aimbit already targets the stable API v2.
- **`X-Upstream-Url` means outbound calls to any https host a client names**, with that client's
  token. Strip the header at your proxy and rely on `--base-url` if that is not acceptable.
- New, with no users yet.

## Where a competitor is better

- **aimbitgmbh/vikunja-mcp**: most of Vikunja's API (projects, labels, relations, buckets,
  notifications) on API v2, with deletes off by default. If the assistant should manage the
  board's structure, not only its tasks, it does more, at the cost of 52 tools in every request.
- **Todoist's and Linear's own servers**: vendor-maintained, with the vendor's guarantees;
  Linear's read-only endpoint is the model emperor-vikunja's `--read-only` follows.

## When to pick something else

- You want the assistant to create and reorganise projects, buckets or relations: aimbit's server.
- You are not on Vikunja: the vendor's own server for your tracker.

Pick emperor-vikunja when the assistant should work with tasks (find, read, create, update,
comment) on a short tool list a small model can handle, with a read-only mode and no way to
delete, or when one HTTP server serves several people with their own tokens.

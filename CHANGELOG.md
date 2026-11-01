# Changelog

All notable changes to emperor-vikunja. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow [semantic versioning](https://semver.org/).

## [Unreleased]

## [0.1.2] - 2026-11-01

### Added
- **`VIKUNJA_READ_ONLY`** - read-only mode can be switched on from the environment (`true`, `1` or `yes`; unset, empty, `false`, `0` or `no` leave it off), for launchers that only set environment variables, such as the Docker MCP Catalog. The startup log and `doctor` show the mode.
- **Container image checked on amd64 and arm64** - the image is built natively on both architectures and spoken to over stdio against a real Vikunja.

### Changed
- **MCP Registry metadata** - `serve`, `--stdio` and `--read-only` are fixed positional arguments; `VIKUNJA_READ_ONLY` is listed as an optional environment variable.

## [0.1.1] - 2026-11-01

### Added
- **Container image** - a `Dockerfile` that builds a small non-root image serving stdio; the Vikunja address comes from `VIKUNJA_URL`, the token from `VIKUNJA_TOKEN`.
- **MCP Registry metadata** - `server.json` describes the crate, its stdio arguments and its environment variables for the official MCP Registry; the README carries the registry name.

## [0.1.0] - 2026-11-01

> First release: Vikunja tasks for any MCP client.

### Added
- **Six tools** - `list_tasks` (open tasks by default, sorted by priority then due date; filters for project, label, done, due date and text), `get_task` (every field, description as text, the most recent comments), `create_task` (Markdown description, labels, assignees, priority, due date), `update_task` (title, description, done, priority, due date, labels, assignees, move to another project), `comment` (Markdown) and `list_projects` (parent and open task count).
- **Tool annotations** - read tools are marked read-only; `create_task` and `comment` as additive; `update_task` as destructive and idempotent; all as open-world.
- **`--read-only`** - serves only `list_tasks`, `get_task` and `list_projects`; the write tools are removed from the server, and `doctor` shows the mode.
- **Labels and assignees by name** - labels by title, created only when `create_missing_labels` is true; assignees by username. Names are checked before anything is written.
- **`X-Upstream-Url` header** - each HTTP request can name its Vikunja instance (https, or http on loopback), falling back to `--base-url`, which is now optional over HTTP.
- **Token rejection reaches the client** - when Vikunja answers 401 or 403, the MCP response over HTTP carries that status (and `WWW-Authenticate` on 401), so clients can ask for a new token.
- **Output caps** - at most 100 tasks or projects per call, descriptions cut at 20 000 characters, the 50 most recent comments at 4 000 characters each.
- **Shared server flags** - `--allowed-host` and `--allowed-origin` for in-process Host and Origin checks.
- **Streamable HTTP and stdio** - stateless HTTP at `/mcp` with `/health` on emperor-mcp; stdio for desktop clients.
- **Token forwarding** - over HTTP every request brings its own `Authorization: Bearer` token, which goes to Vikunja and is not kept; in stdio mode the token comes from `VIKUNJA_TOKEN`.
- **Framed output** - task titles, descriptions and comments reach the model marked as messages written by people for the user, project listings and confirmations as computed data.
- **`doctor` subcommand** - base URL, Vikunja API and version, token and login, mode, listen address, tool budget; exits 1 on failure.
- Release binaries for Linux x86_64, macOS arm64 and x86_64, and Windows x86_64, with a `SHA256SUMS` file and build provenance attestations.

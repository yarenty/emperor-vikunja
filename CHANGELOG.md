# Changelog

All notable changes to emperor-vikunja. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow [semantic versioning](https://semver.org/).

## [Unreleased]

## [0.1.0] - 2026-11-01

> First release: Vikunja tasks for any MCP client.

### Added
- **Six tools** - `list_tasks` (open tasks by default, sorted by priority then due date; filters for project, label, done, due date and text), `get_task` (every field, description as text, comments), `create_task` (Markdown description, labels created on first use, priority, due date), `update_task` (title, description, done, priority, due date, labels, move to another project), `comment` (Markdown) and `list_projects` (parent and open task count).
- **Streamable HTTP and stdio** - stateless HTTP at `/mcp` with `/health` on emperor-mcp; stdio for desktop clients.
- **Token forwarding** - over HTTP every request brings its own `Authorization: Bearer` token, which goes to Vikunja and is not kept; in stdio mode the token comes from `VIKUNJA_TOKEN`.
- **Framed output** - task descriptions and comments reach the model marked as reference data, listings as computed data.
- **`doctor` subcommand** - base URL, Vikunja API and version, token and login, listen address, tool budget; exits 1 on failure.
- Release binaries for Linux x86_64, macOS arm64 and x86_64, and Windows x86_64.

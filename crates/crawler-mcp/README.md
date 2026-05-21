# crawler-mcp

Model Context Protocol server for PlausiDen-Crawler. Exposes the
`crawler` subcommand surface as JSON-RPC tools so MCP-aware
clients (Claude Code, Codex, Cursor) can run reference-captures,
journeys, and detector scans without re-parsing CLI text.

Per paul 2026-05-21 — surviving `cargo clean` and standardising
the screenshot+detect+diff loop is the highest-leverage
improvement after `forge-mcp` (PlausiDen-Forge PR #26).

## Install

```sh
cargo install --path crates/crawler-mcp --bin crawler-mcp
```

The `crawler` binary itself must also be on `PATH` (the MCP
server shells out to it). Install with:

```sh
cargo install --path crates/crawler-runner --bin crawler
```

## Use from Claude Code

Add to `~/.claude/mcp-servers.json`:

```json
{
  "mcpServers": {
    "crawler": {
      "command": "crawler-mcp"
    }
  }
}
```

## Tool surface

### Shipped

- `crawler.capture_reference { url, site_slug?, out_dir? }` —
  390 / 768 / 1280 reference-capture matrix. Returns the
  CaptureManifest JSON.

### Planned

- `crawler.journey { journey_path, out_dir? }` — run a typed
  journey + return findings.
- `crawler.detect { url, detectors[] }` — subset detector run.
- `crawler.diff { reference_dir, current_dir }` — pixel + DOM
  diff between two captures.

# Example CI workflows

Drop these into the `.github/workflows/` of any downstream consumer
(PlausiDen-Loom, PlausiDen-Forge, any site you want to monitor).

## `supersociety-audit.yml`

Runs the PlausiDen-Crawler against a journey on every push +
pull-request, uploads the HTML dashboard + SVG badge as artefacts,
comments the score on PRs, and (on main branch) commits the latest
badge so README embeds stay current.

### Quick start

1. Copy `supersociety-audit.yml` into your repo at
   `.github/workflows/supersociety-audit.yml`.
2. Edit the `env:` section near the top:
   - `AUDIT_JOURNEY` — point at your journey JSON.
   - `SITE_STARTUP_COMMAND` — how to start your site (e.g.
     `python3 -m http.server 8123 --directory dist &` for
     a Forge-built static site).
   - `SITE_HEALTH_URL` — the URL the runner polls until the
     site responds.
   - `BADGE_COMMIT_PATH` — where the latest badge gets
     committed on main (default `badges/supersociety.svg`).
3. Embed the badge in your README:
   ```markdown
   ![Supersociety](badges/supersociety.svg)
   ```
4. Push. The first run on main will commit the badge; subsequent
   runs refresh it.

### What the PR comment looks like

```
## Supersociety Audit — `skillshots-poc`

**Grade A (100/100)** — Supersociety baseline met — zero findings
across all categories.

| category | score | grade | strict | warn |
|----------|-------|-------|--------|------|
| transportSecurity | 100 | A | 0 | 0 |
| originIsolation | 100 | A | 0 | 0 |
| contentSecurity | 100 | A | 0 | 0 |
| ...

Full HTML dashboard available as a workflow artefact.
```

### Customisation points

- **Failure thresholds**: edit the `Fail the build on regression`
  step's `jq` expression to fail on lower grades (e.g. `[ "$GRADE" = 'F' ] || [ "$GRADE" = 'D' ]`) or composite drops.
- **Self-hosted runners**: change `runs-on: ubuntu-latest` to your
  runner labels.
- **Multi-journey**: copy the `audit` job + give each a unique
  `name:` and `AUDIT_JOURNEY` env var.
- **Pinning the crawler**: change `CRAWLER_REF` from `master` to a
  specific tag or commit SHA to avoid detector-change drift breaking
  regression detection. Recommended for any production audit pipeline.

### Permissions

The workflow needs `contents: write` (for the badge-commit-on-main
step) and `pull-requests: write` (for the PR-comment step). Both are
declared at the top of the workflow file. If your org policy blocks
`contents: write`, omit the badge-commit step and embed via the
artefact-download URL instead.

### Notes

- The crawler runs on Linux Chromium via Playwright. The `Install
  Playwright browser` step pulls ~150 MB on first run; subsequent
  runs use the GitHub Actions cache automatically.
- The journey JSON must be reachable from the consumer's working
  dir. The example resolves it as `$GITHUB_WORKSPACE/$AUDIT_JOURNEY`.
- The `CRAWLER_COMMIT_SHA` env var stamps each score-history entry
  with the consumer's commit, letting regressions trace back to a
  specific PR.

# Supersociety Data-Loss Prevention — design + ops manual

Companion to `SUPERSOCIETY_OBSERVABILITY.md` (cycle 77). Where
that doc covers the 6-layer security-telemetry pipeline, this
one covers the **4-layer operator-data-loss prevention ladder**
built across T76 cycles 79-82 in PlausiDen-Loom.

The supersociety doctrine treats operator data loss as a
**security-class concern**: a system that silently loses an
operator's typing is untrustworthy. Every layer here exists
because a real user-facing failure mode would otherwise
destroy work.

---

## TL;DR

Four independent defense layers cover three distinct
data-loss vectors:

```
Vector 1: BROWSER-LEVEL                            Cycle 79
  closes / refreshes / navigates away with unsaved input
       ↓
  Cmd-S keyboard save + dirty indicator (● in title) +
  beforeunload navigation warning

Vector 2: MID-EDIT (browser crash, network drop, OOM)    Cycle 82
  loses everything in the form because the user never
  clicked save before the crash
       ↓
  localStorage draft autosave every 500ms +
  restore-or-discard banner on next page load

Vector 3: FILE-LEVEL (regret, paste-over, fat-finger)    Cycle 80
  the save succeeded but it was the wrong save
       ↓
  Auto-snapshot to cms/<slug>.bak.<unix>.<nanos>.json
  before every overwrite, with LRU retention

Layer 4: OPERATOR UX for vector 3                  Cycle 81
       ↓
  loom revisions list / show / diff / restore <slug>
```

Every layer:
- Ships as production code (not a TODO).
- Has E2E tests pinning its behaviour (6 tests for revisions;
  property + mutation + drift checks across the score module).
- Maintains the supersociety security stack (CSP hash-pinned,
  Trusted-Types clean, defense-in-depth headers, audit-clean
  at A 100/100 across all 16 surfaces).

---

## The doctrine: data loss = security regression

Why is this in a "supersociety" doctrine doc, not a UX changelog?

Because the threat model is **untrustworthy operator
experience**. If I'm content-editing for hours, then a stray
back-button drops my work, I'll never trust the tool again. The
system has betrayed me. From a behavioural-security perspective,
it doesn't matter if the cause was a CVE or a missing
`beforeunload` handler — both make my work disappear without my
consent.

Same defense-in-depth doctrine applies:
- **Detect** that data is at risk (input events → dirty flag).
- **Enforce** safety (Cmd-S, autosave, beforeunload prompt).
- **Report** what happened (dirty indicator in title, restore
  banner).
- **Collect** evidence (localStorage drafts, .bak files).
- **Audit** the pipeline (E2E tests pinning every layer).
- **Review** & **Recover** (loom revisions).

Mirror of the cycle 77 observability stack, applied to a
different threat axis.

---

## Layer 1: BROWSER-LEVEL  (cycle 79)

**Where**: `EDIT_PAGE_JS` const in `loom-cli/src/main.rs`
(server-rendered inline script on every edit-form page).

**Trigger**: any `input` event from an `<input>` / `<textarea>` /
`<select>` inside the `.editor` pane.

**Effect**: sets an in-memory `dirty=true` flag.

### Side effects of `dirty=true`

1. **Visible indicator**: `document.title` gets a leading `●`.
   Matches the convention of every other content editor on the
   planet (VSCode, Sublime, ChatGPT). The operator sees the
   indicator in the tab bar even with the page scrolled away.

2. **Navigation guard**: `window.beforeunload` shows the
   browser's native "Leave page? Changes you made may not be
   saved" prompt. The browser is the only authority that can
   intercept tab-close / refresh / back-button reliably; we
   tell it to ask before letting any of those steal the work.

3. **Cleared on submit**: `dirty=false` on form `submit` event.
   The cycle 79 `setDirty(false)` removes the `●` and silences
   `beforeunload`.

### Cmd-S / Ctrl-S handler

A `keydown` listener fires `form.requestSubmit()` (or
`form.submit()` fallback) on Cmd-S / Ctrl-S. Modifier-strict —
Shift, Alt cancel the handler so accidental chords don't fire.

```javascript
document.addEventListener('keydown', function(e) {
  if ((e.ctrlKey || e.metaKey) && !e.shiftKey && !e.altKey && e.key === 's') {
    var form = findEditorForm();
    if (form) {
      e.preventDefault();
      form.requestSubmit ? form.requestSubmit() : form.submit();
    }
  }
});
```

### What this layer DOES NOT protect against

- Browser process crashes (the `beforeunload` event never fires
  on a crash). → Vector 2 / cycle 82.
- The operator clicking Save then realising it was wrong. →
  Vector 3 / cycle 80.
- File-system corruption after save. → Out of scope; relies
  on cycle 71's collector-rotation-pattern at the storage
  layer.

---

## Layer 2: MID-EDIT  (cycle 82)

**Where**: same `EDIT_PAGE_JS` const, appended to the cycle 79
script.

**Trigger**: `input` events (debounced 500ms).

**Effect**: snapshots the form's current field values to
`localStorage` under the key `loom-draft:<slug>`.

### Storage shape

```json
{
  "savedAt": 1736380800123,
  "fields": {
    "title": "Half-court arc",
    "description": "Single arc, no rebound, no backboard.",
    "sec.0.eyebrow": "Open battle",
    "sec.0.title": "…",
    "sec.0.lede": "…",
    "sec.0.cta": ""
  }
}
```

`savedAt` is `Date.now()` (unix milliseconds). `fields` is a
flat map of every named form input (excluding `hidden` /
`submit` / `button` types).

### TTL

7 days. Read-time check on page load:

```javascript
if (Date.now() - draft.savedAt > 7 * 24 * 60 * 60 * 1000) {
  localStorage.removeItem(DRAFT_KEY);
  return null;
}
```

A draft older than 7 days is silently discarded. Prevents
indefinitely-stale entries from haunting a slug after weeks
of inactivity.

### Restore banner

On page load, if a draft exists and is within TTL, the editor
pane inserts a yellow `<div role="status">` banner at the top:

```
┌──────────────────────────────────────────────────────────────────┐
│  Unsaved draft from 3m ago — 5 field(s) staged in your browser.  │
│                                              [ Restore ]  [ Discard ] │
└──────────────────────────────────────────────────────────────────┘
```

- **Restore**: walks `form.querySelectorAll('[name=X]')` and
  rehydrates field values. Sets `dirty=true` so the cycle 79
  indicator fires.
- **Discard**: removes the localStorage entry.

Banner is built with `document.createElement` + `appendChild`
+ `textContent`. **No `innerHTML` anywhere on this path** —
required because the page emits
`require-trusted-types-for 'script'` (cycle 57); using
`textContent` bypasses the Trusted Types sink surface
entirely. Honest by construction.

### What this layer DOES NOT protect against

- Cross-device editing (localStorage is per-browser-per-origin;
  another machine sees no draft).
- A second tab editing the same slug racing against the first.
  Last-writer-wins on localStorage; the form-submit clears the
  draft after a save, so the second tab loses its local
  copy. Mitigated indirectly by the cycle 80 file-level
  backup.
- An operator who clicks Discard then changes their mind. The
  localStorage entry is gone; only the cycle 80 file-level
  backup can help.

---

## Layer 3: FILE-LEVEL  (cycle 80)

**Where**: `save_cms_revision()` in `loom-cli/src/main.rs`,
called inline before every `cap.write_atomic()` on a CMS JSON
file.

**Trigger**: 4 mutation paths in the server:

| Handler | What triggers it |
|---|---|
| `handle_edit_post` | full-page form save (the big Save button) |
| `handle_add_section` | "Append" button on the section composer |
| `handle_inline_edit` | cycle T62 click-to-edit single-field save |
| `handle_section_op` | move-up / move-down / delete / append-paragraph |

`handle_new_page` is deliberately skipped — it's creation, not
edit; no prior content to back up.

### File format

```
cms/
  about.json                              ← live, mutable
  about.bak.1736380800.123456789.json     ← rev N (newest)
  about.bak.1736381900.456789123.json     ← rev N-1
  about.bak.1736382000.789123456.json     ← rev N-2
  …
```

Backup naming: `<slug>.bak.<unix_secs>.<nanos>.json`.

Fixed-width `unix_secs.nanos` suffix means **lexical sort =
chronological sort** — cycle 71's collector-rotation
convention applied to data backups. Pruning the oldest is just
taking the first N entries.

### Retention

Default 10 revisions per slug. LRU prune — when a new save
arrives and there are already 10 revisions, the oldest is
deleted before the new one is written. The active file is
NEVER pruned.

Tunable via `LOOM_CMS_REVISIONS_KEEP` env. Operators normally
rely on the default.

### Failure modes

Backup write failure (full disk, permission denied, etc.)
**logs to stderr but NEVER blocks the save itself**. Lost
revisions are a backup problem; a lost SAVE is intolerable.

Same doctrine as cycle 69's rate-limited collector (drop
silently, never retry-storm) and cycle 70's report-tail
viewer (best-effort, never fail).

---

## Layer 4: OPERATOR UX  (cycle 81)

**Where**: `cmd_revisions()` in `loom-cli/src/main.rs`.

**Trigger**: operator runs `loom revisions <action> <slug>
[N]`.

### Four actions

```
loom revisions list <slug>            see what's available
loom revisions show <slug> [N]        print revision N content
loom revisions diff <slug> [N]        diff revision N vs active
loom revisions restore <slug> [N]     atomic restore
```

Index is 1-based. `N=1` = most-recent backup. Defaults to 1
when omitted.

### Restore is reversible

The cycle 80 backup ladder has **no terminal step**: restoring
revision N FIRST takes a snapshot of the current active file,
THEN replaces it with revision N. If the operator restores the
wrong revision, they can `list` again and restore the snapshot
that was just created.

```
$ loom revisions list home
  n  when                       bytes  filename
  1  2026-05-14 17:01:35Z         221  home.bak.1778778095.398599254.json
  2  2026-05-14 16:58:24Z         221  home.bak.1778777904.091001370.json
  …

$ loom revisions diff home 1
--- home.bak.1778778095.398599254.json (revision 1)
+++ home.json (active)
-  "title": "Cycle 80 save 9"
+  "title": "Cycle 80 save 14"

$ loom revisions restore home 1
loom revisions restore: 'home' restored from revision 1 (219 bytes)
(the prior active content was snapshotted as a new backup;
 run `loom revisions list home` to confirm)
```

### Implementation discipline (same as cycle 70/72)

- **Hand-rolled unified diff**. Line-set membership; no
  external `diff` binary or library. Coarser than Myers diff
  (it shows every removed and added line, doesn't preserve
  multiplicity), but honest about its bluntness in the
  output. Adequate for CMS JSON file restores.
- **Shared date formatter** (`report_log_format_unix`) with
  the cycle 70/72/76 report subcommands. Same human format
  across every viewer command in loom-cli.
- **Atomic restore**: write to a temp file in the same dir,
  then rename. Mirrors cycle 60's `WriteCapability::
  write_atomic`. The restore either fully succeeds or has
  no visible effect.

---

## Operating the system

### "I accidentally closed the tab"

Cycle 79's `beforeunload` warning fires. Click "Cancel" /
"Stay on page". Your typing is intact.

### "The browser crashed before I clicked Save"

Open the same edit URL. Cycle 82's restore banner appears at
the top of the editor:

```
Unsaved draft from 3m ago — 5 field(s) staged in your browser.
[ Restore ]  [ Discard ]
```

Click Restore. Your typing comes back. Click Save when you're
done.

### "I saved by mistake and want to undo"

```
loom revisions list <slug>          # see what's available
loom revisions diff <slug> 1        # see what changed
loom revisions restore <slug> 1     # 1 keystroke restore
```

Restore is itself reversible — your "wrong save" is
snapshotted before the restore overwrites the active file.

### "How many backups exist for this page?"

```
loom revisions list <slug>
```

Up to 10 by default. `LOOM_CMS_REVISIONS_KEEP=20 loom edit-serve`
to bump retention.

### "Disk space is filling up; where's the backup data?"

```
ls -lh cms/*.bak.*.json
```

All under `cms/`, named `<slug>.bak.<unix>.<nanos>.json`.
Sortable lexically = chronologically. Delete any safely; the
active `<slug>.json` is the live state.

---

## What this still isn't

- **No cross-device sync** of localStorage drafts. They live
  in one browser, one origin. Acceptable for the current
  single-operator threat model; would change if PlausiDen
  shipped multi-user editing.
- **No conflict detection** on concurrent edits. If two
  operators open the same slug at the same time and both
  save, the second save wins. Mitigated by cycle 80's file-
  level backup (the first save lives on as a `.bak` file
  and can be restored / merged manually).
- **No structured diff**. Cycle 81's `diff` is line-set,
  not minimal-edit-script (Myers). For a JSON file this is
  often adequate because the JSON is pretty-printed —
  line-level granularity matches operator mental model.
  A future cycle could add a JSON-aware semantic diff.
- **No `--restore-from <timestamp>`** alternative to the
  1-based index. Operators have to `list` first to find the
  revision they want. Future cycle could add timestamp-based
  picking.
- **localStorage drafts are not encrypted**. A user with
  local browser access can read them. Acceptable for the
  current threat model (the same user is already editing the
  CMS); not acceptable if drafts could contain PII.

These are explicit, named gaps. AVP-2 doctrine: ship explicit
risk acceptance, not silent omission.

---

## File index

| File | Cycle | Purpose |
|---|---|---|
| `PlausiDen-Loom/loom-cli/src/main.rs::EDIT_PAGE_JS` | 79+82 | Browser + mid-edit defense JS |
| `PlausiDen-Loom/loom-cli/src/main.rs::save_cms_revision` | 80 | File-level backup writer |
| `PlausiDen-Loom/loom-cli/src/main.rs::prune_cms_revisions` | 80 | LRU pruning |
| `PlausiDen-Loom/loom-cli/src/main.rs::cmd_revisions` | 81 | Operator UX |
| `PlausiDen-Loom/loom-cli/tests/revisions_e2e.rs` | 81 | 6 E2E tests pinning revisions |
| `PlausiDen-Crawler/docs/SUPERSOCIETY_OBSERVABILITY.md` | 77 | Companion doc (security telemetry) |

Cumulative as of cycle 82: 36 Loom commits + 13 crawler
enhancements. The data-loss prevention ladder is 4 cycles + 1
doc — a complete, named, tested, documented feature surface.

---

*This document is part of the supersociety doctrine artifact
set. If you're a content editor wondering why your typing is
safe, read this. If you're a maintainer adding a new mutation
path, make sure it goes through `save_cms_revision` BEFORE
calling `cap.write_atomic`.*

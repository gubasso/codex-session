# `.plan/` — implementation plan queue

This directory holds `codex-session`'s implementation plans. It is **flat and
queue-driven**: status, order, dependencies, and the execution command live in
YAML queue files — never in directory or file names.

## Layout

```text
.plan/
├── _README.md                 # this file
├── _QUEUE.yaml                # top-level source of truth: every plan + status
├── <plan-slug>/               # a multi-round plan (a directory)
│   ├── _README.md             # plan overview, strategy, decisions
│   ├── _QUEUE.yaml            # this plan's rounds, in execution order
│   └── <round-topic>.md       # one self-contained round (no number prefix)
└── <plan-slug>.md             # a single-file plan (a loose note)
```

There are **no `01-todo/` / `02-done/` kanban directories** and **no `NN-`
number prefixes**. A plan's lifecycle state is its `status` field, not its path.

## The queues are the source of truth

- **`.plan/_QUEUE.yaml`** lists every plan (full ledger, including completed
  ones). Each entry has: `item`, `status`, `depends_on`, `prompt`, `notes`.
- Each multi-round plan dir has its own **`_QUEUE.yaml`** listing its `rounds`
  in execution order, with the same fields.

`status` is one of: `backlog | todo | doing | done`.

Order in `_QUEUE.yaml` reflects execution priority: active items first
(`doing`/`todo`), then `backlog`, then `done`. Re-order entries freely to
re-prioritize — there are no filenames to rename.

## Executing a plan

Each entry carries a `prompt` — the exact command to run it:

- **Multi-round plan:** `prompt` is `/prex -ar @.plan/<slug>/`. The executor
  reads that plan's inner `_QUEUE.yaml`, runs the **first `todo` round**, then
  stops. One round per `/prex` session — never chain rounds in one session.
- **Single-file plan:** `prompt` is `/prex -ar .plan/<slug>.md`.

## Updating status (replaces the old `mv` lifecycle)

When a round finishes, set that round's `status: done` in the plan's inner
`_QUEUE.yaml`. When all rounds are done, set the plan's `status: done` in the
top-level `.plan/_QUEUE.yaml`. Nothing moves on disk — the plan dir stays put;
only the `status` fields change.

## Authoring new plans

New plans are written by the `plan-writer` skill, which emits this exact
structure (flat `<slug>/` dir, `_README.md` + `_QUEUE.yaml`, de-numbered round
files) and appends the plan to `.plan/_QUEUE.yaml`.

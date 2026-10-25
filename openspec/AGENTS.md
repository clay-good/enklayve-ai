# OpenSpec conventions for Enklayve

This project uses a lightweight, spec-driven workflow.

## Layout

```
openspec/
  project.md              Project context, principles, non-goals (read first)
  AGENTS.md               This file
  specs/<capability>/     The durable contract: what Enklayve does, today's truth
    spec.md
  changes/<change-id>/     Proposals for work not yet shipped
    proposal.md           Why + what changes (one page)
    tasks.md              Ordered, checkable implementation steps
    design.md             Optional: technical decisions and trade-offs
    specs/<capability>/   Spec deltas: ## ADDED / ## MODIFIED / ## REMOVED
```

## Spec format

Each `spec.md` has a `## Purpose` then `## Requirements`. Every requirement is a
`### Requirement:` written as a SHALL statement, followed by one or more
`#### Scenario:` blocks in WHEN/THEN form. Scenarios are the acceptance tests —
keep them concrete and verifiable.

## Workflow

1. Before building, read `project.md` and the relevant `specs/`.
2. For new or changed behavior, add a change under `changes/<change-id>/` with a
   `proposal.md`, `tasks.md`, and spec deltas. Keep each change a shippable unit.
3. When the change ships, fold its deltas into `specs/` and archive the change.

The specs in `specs/` currently describe the **target** vision (see
`ROADMAP.md` for sequencing). As capabilities ship, they become plain truth.

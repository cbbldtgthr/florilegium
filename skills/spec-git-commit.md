---
source_sha: 9039640
marshalled_at: 2026-10-24T02:34:27Z
---


# spec-git: Commit grilling

## Why this system exists

The agent has full session context of what was just implemented and why. After a long
session, intent is at peak clarity for the agent and peak risk of being lost to a quick
commit one-liner. This skill inverts that: the agent drafts the narrative and grills the
developer to validate and complete it, before the commit is made.

The human holds the intent. The agent is the scribe. The human is the editor.

## How the message is read

A future agent reads this message through tool calls, next to the diff — often after
`git blame` on a few lines led it there. It pays tokens for every line it reads. Write
for that reader:

- The first sentence gives the why, and stands on its own without other commits loaded.
- Anything the diff already shows is left out (see below).
- A cross-reference asks the reader to fetch another commit. Earn it, and summarise the
  link in one clause so they usually need not.
- The default shape is a headline and one short paragraph. Extra lines are exceptions
  that justify themselves.

## Steps

After work is complete, before any `git commit`:

1. **Draft** — Write a proposed commit body from session context. Do not ask permission
   first; draft immediately.
2. **Grill** — Ask all five structured questions below in a single message.
3. **Revise** — Incorporate the answers; correct anything that contradicts stated
   intent. Then cut per **What to leave out**: manifests, mechanism, colour, restatement.
4. **Trim the backlog** — If this change implements (or partly implements) an item in
   `BACKLOG.md`, stage the edit that removes or trims that entry **as part of this same
   commit** (`constitutions/spec-git.md`: double-entry). Delete a fully-delivered entry;
   trim a partly-delivered one to what still remains. If nothing in the backlog is
   touched, skip this step — do not invent an entry to delete.
5. **Present** — Show the complete proposed commit message (headline + body), and name
   the backlog entry being removed or trimmed, if any.
6. **Approve** — Wait for the user to confirm, edit, or reject. Never commit before
   approval.

## Grilling questions

Ask all five at once. Brief answers are acceptable; the goal is accuracy, not prose.

1. What is the **job-to-be-done** — the need this serves, not the mechanism?
2. What alternatives or approaches were **ruled out**, and why?
3. What **prior decisions** does this build on, supersede, or intentionally depart from?
4. What **constraints** shaped the approach (technical, time, compatibility, resourcing)?
5. What **open questions or follow-ons** remain — known debts or deliberate deferrals?

## Commit body format

```
<headline: imperative, <= ~60 chars, grep-friendly>

<Why this change exists: 1-3 sentences, readable with no session context and no
other commits loaded. This is what a future blame on the changed lines must surface.>

Ruled out: <alternative> - <one-clause reason>.   [only real alternatives]

<Lineage: <hash> - <what it established and how this revises it>.   [see below]

Open: <debt or deferral a maintainer must know>.  [omit if none]
```

Most commits are the headline plus the one paragraph. Every optional line is omitted
unless it carries intent the diff cannot.

## Lineage

When a change **revises, reverses, or narrows a decision a past commit made on
purpose**, cite that commit by hash and say in one clause what it established and
why the thinking moved. This threads a loose lineage through the log — a reader on
any one of these commits can walk the chain and see the reasoning evolve, not just
the final state.

- Use the verb that fits: `Reverses <hash>`, `Narrows <hash>`, `Supersedes <hash>`,
  `Builds on <hash>`.
- One clause, resolvable: `Reverses a1b2c3d, which made kind an enforced vocabulary
  - convention now belongs in the skill, not the parser.`
- Cite the commit that **owns the decision** (usually where it was last deliberately
  set), not every commit that touched the lines. If several, list the two that matter.
- Only for deliberate reversals of intent. A routine follow-up that just extends the
  previous commit does not need a lineage line - the diff and adjacency say enough.
- At session start, `spec-git-startup` surfaces the relevant prior commits; use those
  hashes.

## What to leave out

The reader has the diff in the same view. The body is only for what the diff cannot
show. Cut, every time:

- **File and directory manifests.** The diff lists the files. Describe the decision, not
  the tree.
- **How it is built or structured.** Edition, dependency names, module layout, flag
  lists - all in the diff. Name a mechanism only when the *reason* for it is the point.
- **What the headline already frames.** The headline's verb - Scaffold, Bump, Rename,
  Extract, Wire - sets expectations. Do not spend the body re-establishing that a
  scaffold is boilerplate or a bump is routine. Write only what is surprising *given*
  the headline.
- **Etymology and background colour.** One line of rationale for a name or choice. No
  stories.
- **Restatement.** A sentence that paraphrases a constitution, skill, or existing doc
  adds nothing - the reader can open it.
- **Default non-choices.** "Ruled out" is for a path that was genuinely on the table and
  lost for a specific reason. Not the defaults every practitioner skips without
  deliberating - no workspace for a single crate, no dependency you never reached for.

Test each sentence: if it is equally true of another commit, or copied from something
already in the repo, it is not intent - delete it.

## Self-instruction

Invoke before every commit. Never commit first and describe later — session context
required for an accurate narrative is gone once the conversation closes. The backlog
trim (step 4) rides in the same commit, never a follow-up.



---
source_sha: 9039640
marshalled_at: 2026-10-24T02:34:27Z
---


# spec-git

Every project has two representations of its state: the **code** (the materialized
table — current truth) and the **commit history** (the log — immutable, append-only
record of how it got there). spec-git makes the log carry the spec.

The code holds **implementation authority**. The commit narrative holds **intent
authority**. These are different things. A separate English-language spec document
creates two sources of truth, which eventually means zero reliable sources of truth.

Rules:

- **Commit headline**: standard best practice — imperative mood, short, grep-friendly.
  Nothing changes here.
- **Commit body**: spec narrative — job-to-be-done, what was ruled out, what prior
  decisions this supersedes or relates to (by hash). Cross-references are permanently
  resolvable via `git show <hash>`.
- **Lineage.** When a commit reverses or narrows a decision an earlier commit made on
  purpose, it cites that commit by hash and says how the thinking moved. The chain of
  such references is the record of *why the design changed*, not just what it is now.
  See [[spec-git-commit]].
- Never maintain a parallel spec of what the code **currently does** — that is the
  code's job and the commit log's.
- **`BACKLOG.md`** is the one document that looks forward: work that has been specced
  far enough to start but is **not yet built**. It is not a spec of current state; it
  is a queue.
- **Double-entry.** The commit that implements a backlog item **deletes that item from
  `BACKLOG.md` in the same commit**. The addition (code + narrative) and the retirement
  (backlog deletion) are one atomic record. Partial delivery trims the entry to what
  remains rather than removing it. An empty section is fine; an empty `BACKLOG.md` means
  the queue is clear.
- It is **not strict** double-entry: not all work is queued first. A commit with no
  backlog change is asserting the work was an in-the-moment idea, not a planned one.
  That is legitimate — never back-fill an entry just to delete it in the same commit.
- **`README.md`** holds the overarching *why* — the vision and principles that do not
  change as features land. Not a feature list.
- At session start, negotiate context from the commit history before writing code.
  See [[spec-git-startup]].
- At commit time, the agent grills to capture intent before it evaporates, and trims
  the backlog in the same commit. See [[spec-git-commit]].



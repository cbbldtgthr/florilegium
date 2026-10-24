---
source_sha: 9039640
marshalled_at: 2026-10-24T02:34:27Z
---


# spec-git: Session startup and context negotiation

## Why this system exists

Loading the full commit history into context is expensive and usually wrong — most of it
is irrelevant to the current prompt. This skill retrieves only the commits whose
narratives bear on the current task, then surfaces that context explicitly so the user
can correct or supplement it before work begins.

The deep read — full commit bodies and diffs, potentially many and large — runs in a
**subagent with its own context window**. Only its digest crosses back into the main
context; the raw history is never paid for there.

This externalises the agent's assumptions *before* they bake silently into the
implementation. It also forces the user to confront whether the new prompt is consistent
with prior decisions — surfacing productive tensions before they become silent overwrites.

## Steps

Run these before starting any non-trivial task in a spec-git project:

1. **Orient** — in the main context: `git log --oneline -30`. Cheap; gives a timeline
   and the vocabulary to search on. If there are no commits yet, say so and proceed.
2. **Delegate** — dispatch a subagent for the deep read. Give it the current task in one
   or two sentences, and the key terms and concerns to search on. Instruct it to run
   `git log --grep=<term> --format="%H %s%n%b"` per term, then `git show <hash>` on the
   few most relevant, and to **return only the digest below — no diffs, no full bodies,
   no per-commit log.**
3. **Surface** — present the subagent's digest to the user in the output format below.
   Do not begin implementation until the user responds.
4. **Incorporate** — fold in corrections and additions, confirm understanding, begin
   work.

## Subagent return format

> For each materially relevant commit:
>
> - `<short-hash>` — *headline* — 1-2 sentences: the intent this commit established, and
>   why it bears on the task.
>
> If nothing is materially relevant, say so. Do not fabricate relevance.

## Output format

> Based on your prompt, here is my context read:
>
> - `<short-hash>` — *headline* — what intent this commit established, and why it is
>   relevant to this prompt.
> - `<short-hash>` — *headline* — [...]
>
> Does this match your understanding, or is there prior context I am missing?

## Self-instruction

Invoke at the start of every non-trivial session. A trivial session is a one-line fix
where no prior design decision could be affected. When in doubt, run the retrieval.



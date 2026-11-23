# Design issues

One document per question the library has not settled, or has settled by
deciding to add nothing. Each one is written so that the question can be
picked up later without the conversation that raised it.

| Issue | Status |
| --- | --- |
| [A value whose meaning depends on its neighbour](neighbour-dependent-value.md) | open; a rule on the field today, one alternative tried |

## What a document holds

- **The problem**, with an example that is not taken from any one application.
  The library is general, so its examples are.
- **What the pattern is called** elsewhere, so that it can be looked up.
- **Why the library cannot settle it by itself**, if that is the case, and how
  Rust itself handles it.
- **What works today.**
- **The ways it could be stated or avoided**, each with what it costs.
- **What was tried**, with the results and the limits found. A trial that was
  not implemented is described closely enough to be rebuilt.
- **The decision**, with its date, and what would reopen it.

A picture belongs next to its document, as an SVG file of the same name.

The shorter list of open points is at the end of
[the phase plan](../phases.md#open-points-for-later-phases).

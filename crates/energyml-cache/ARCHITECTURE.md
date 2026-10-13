# energyml-cache: architecture

## Purpose and position

What a package keeps of itself, layer by layer. A face holds one `Cache` per open store; layers (`Layers`: `index`, `colours`, `attachments`) are switched on one at a time and are lazy. The crate sits **above `energyml-geometry` and below every face**, compiles to wasm, and has no native dependency.

Related decisions: [ADR-0009](../../docs/adr/0009-properties-tables-colours.md), [ADR-0010](../../docs/adr/0010-package-index-and-caches.md), [ADR-0016](../../docs/adr/0016-one-crate-one-test-binary.md).

## Rules

**One handle, one implementation** — `Cache::off()` keeps nothing, and there is no second "uncached" version of the code. A layer switched off runs the same code over a slot thrown away after the call (`with_index` builds an index for the question and drops it; the colour ladders take a fresh colour slot). A fast answer that differs from the slow one is the worst outcome, so the audit confronts the two on a reference package and counts what is held (`Cache::held`, colour-walk statistics) instead of timing.

**Colours and attachments derive from the index** — they read its links and drop what its updates disturb. Switching either on switches the index on (`Layers::normalised`); an off index would leave nothing for them to be exact against.

**The dependency goes one way** — geometry does not know this crate. `scene::of_package` receives a colour source as a trait (`scene::Decide`), and `colours::Colouring { cache, store }` is the one faces pass to it. Without a source the scene falls back to the short, index-free ordering, which ignores collections and the model; a caller holding a cache must pass the source, otherwise it colours differently from the page. Never make geometry depend on this crate.

**One colour rule for every face** — `Cache::decision` is the rule; the page, the command line and the scene colour ramps all go through it. When the page answered from the index walks and the command line from the short ordering, the same object had two colours depending on the face. A cross-face test confronts them object by object with a count of coloured objects: equality on a set with no colour would prove nothing. Of the two checks, only the ladders one triangulates; the other shares the decision on both sides and proves only the wiring.

**Slots carry the work of invalidation** — `IndexSlot` supports stepwise building with progress, a partial index, a disturbed-count *per consumer* (adding a consumer adds a value and a count, never reading another's), and tampering hooks for sabotage tests. `ColourSlot` holds the ladders and the merged set; a question asked while another is being answered finds the slot taken and answers from a slot of its own, *without* reading the index's disturbed count (a throwaway slot must not consume what the real one still has to discard).

**Attachment listings stay in the domain crate** — what a listing reads and what discards it is the domain's algorithm and its tests, and the domain cannot depend on a crate above it. This crate only decides whether the cache is kept between two questions.

**Faces decide which layers are on, in one fixed place** — the command line starts with `Cache::off()` and a fixed list of commands switches layers on (index and colours for the commands that colour). Layers are lazy, so switching on one never asked for costs nothing.

## Deliberately not here

No geometry cache, no object cache, no persistence to disk, no decisions about colour beyond calling the domain's rule.

## How it is tested

One test binary. The reference package is the repository's synthetic example model, committed with the repository. `on_and_off.rs` confronts on and off on that package and counts held items; the cross-face agreement test lives with the wasm face, which already depends on this crate (no dev-dependency back to a face, so no cycle).

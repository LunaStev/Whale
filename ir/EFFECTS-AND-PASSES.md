# Observable effects and verified passes (semantics 1)

Execution observes calls, memory writes, volatile reads/writes, lifetime changes,
checks and traps in source IR execution order. A reachable trap stops execution
at its instruction/terminator; earlier observations remain earlier and later
ones do not occur. Return value equality alone is insufficient to justify a
transformation: it must preserve trap reasons/order and required memory/call
observations as well. Ordinary reads can trap or observe writes, so they cannot
be speculated, merged or discarded without additional proofs.

For example, removing an unused modular integer add with no retained source or
SSA use can be safe. Removing `udiv x, 0` or a reachable `trap_if true` changes an
observable outcome even when its result is unused. Moving a write after such a
trap erases an earlier observation. A branch proven untaken can justify removing
its unreachable trap only if the proof also preserves condition evaluation and
all effects; the syntactic fact that a result is unused proves none of this.
The effect API is conservative: `permits_unused_removal` is a necessary condition,
not a complete SSA, source-mapping, aliasing or reachability proof.

`volatile_load` and `volatile_store` are scalar accesses in typed IR format 8.
AST format 5 represents `VolatileLoad(pointer)` and
`VolatileStore {pointer, value}`. Pointer/base evaluation precedes value evaluation.
These accesses keep their count, relative execution order and corresponding
scalar memory operations, including when a load result is unused. An effect-aware
pass cannot combine or eliminate them. The interpreter executes each once with
the same tracked bounds, lifetime, alignment and initialization rules as ordinary
memory. Volatile grants no raw-pointer authority, device mapping, atomicity,
fences or cross-thread synchronization. Ordinary Load/Store representations and
behavior remain unchanged.

## Pass boundary

`pass::Pass` reports its name, whether it changed the module, and explicit
CFG/value/effect invalidation. `run_pass` verifies before and after each pass,
retains structured verifier failures with pass/target/stage context, and rolls
back the full module (including diagnostic metadata) on transform, verification
or reporting errors. Structural comparison checks that the change report is
truthful. Successful mutations conservatively invalidate all three analyses;
unchanged passes preserve caches. Epoch overflow fails atomically.

Analysis caches belong to one module/state pair, carry the state's identity and
epoch, and become unavailable after invalidation. Editing a module outside the
runner requires the caller to discard its analysis state/caches. Verification
checks IR validity, not semantic equivalence: pass implementations still own
SSA use, source identity, alias and observable-effect proofs.

No optimizer pipeline or optimization flag is enabled. The existing O0 no-op
remains a no-op; `zero::pass::ZeroPass` is its explicit runner adapter. Conformance
tests use a small test-only pure-dead-candidate transform, compare returned values
and trap reasons, retain all volatile accesses, invalidate stale caches, and
reject/roll back an intentionally invalid transform.

## 2024-05-28 - Unnecessary String Allocations in Filter Loops
**Learning:** React components containing large inline `.filter` callbacks often recompute string transformations (e.g. `searchTerm.toLowerCase()`) redundantly for every element in the array during every render pass. This is an $O(N)$ overhead per render.
**Action:** When adding or optimizing filtering lists, wrap the output in `useMemo` to skip work entirely on identical renders. Furthermore, extract non-element-specific work (like converting `searchTerm` to lowercase) outside of the loop to eliminate unnecessary allocations.

## 2024-05-29 - Rusqlite Prepared Statements Overhead
**Learning:** Calling `tx.execute(...)` inside a loop in `rusqlite` re-parses and compiles the SQL string on every iteration, leading to significant N+1 overhead.
**Action:** Always call `tx.prepare(...)` outside the loop, and use `stmt.execute(...)` inside. This avoids the recompilation overhead, dramatically improving batch insertion speeds. Wrap the statement preparation and loop in an inner scope `{ ... }` to ensure the statement is dropped before calling `tx.commit()?`.

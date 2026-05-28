## 2024-05-28 - Unnecessary String Allocations in Filter Loops
**Learning:** React components containing large inline `.filter` callbacks often recompute string transformations (e.g. `searchTerm.toLowerCase()`) redundantly for every element in the array during every render pass. This is an $O(N)$ overhead per render.
**Action:** When adding or optimizing filtering lists, wrap the output in `useMemo` to skip work entirely on identical renders. Furthermore, extract non-element-specific work (like converting `searchTerm` to lowercase) outside of the loop to eliminate unnecessary allocations.

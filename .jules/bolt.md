## 2025-02-18 - Avoid synchronous setState within useEffect in Next.js

**Learning:** Next.js 15 strict linting flags `setState` inside `useEffect` (`react-hooks/set-state-in-effect`) as a performance warning to avoid cascading renders. Wrapping it in a `setTimeout` will break React's natural rendering cycle and is an anti-pattern. If you encounter a legacy effect causing this error that you can't architecturally fix within the scope of a small optimization, you may have to silence the linter via an `eslint-disable-next-line` directive to pass the strict `pnpm lint` checks safely.

**Action:** Prefer deriving state or updating it via event handlers instead of `useEffect`. When fixing unrelated issues where legacy code triggers the `react-hooks/set-state-in-effect` linting rule, consider silencing the warning instead of trying to hack around it with `setTimeout`.

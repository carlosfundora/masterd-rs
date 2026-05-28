## 2024-05-28T23:00:00 - Initial Baseline Planning

### Run Mode
planning

### What I Learned
The repository is a mix of a Rust workspace (Cargo) with multiple crates/apps, a Tauri app, a Next.js shell (`masterd-shell`), and some Python services.
Cargo needs `protobuf-compiler`, `libasound2-dev`, `mold` to build properly on this environment.
There are a few `misc`, `utils`, `helpers` directories inside vendor code, but mostly clear in our own codebase except for standard utility files.
`apps/masterd-shell/lib/utils.ts` is standard tailwind setup.

### What I Organized
Created baseline operational directories in `.jules/`.

### Canonicality Decisions
N/A

### References Repaired
N/A

### Risks Found
N/A

### Useful Notes for Next Run
Look into `services` and see if they have clear boundaries. Investigate `masterd-core` and `masterd-data` overlaps.

### Recommended Next Targets
Review `services` directory structure.

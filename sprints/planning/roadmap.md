# Roadmap

> The general plan for this project. Keep it current; detail lives in the
> sprint records. korg (project `kvscf`) is the authoritative queue — this file
> is the readable shape of it.

## Now

- Nothing in flight. Sprints 019 (kprojects harness), 020 (favorites repair) and 021
  (dev-host windows + focus after relaunch) have landed.

## Next

- **Folderless windows.** An ordinary VS Code window with no folder open still reads as workspace
  `Visual Studio Code` in the rail. Sprint 021 fixed this only for the extension-development host
  (korg #627) and deliberately left the general case alone; decide whether those rows should be
  relabelled or dropped.
- **Tell the dashboard about dev hosts.** kvscf's half is done — sprint 021 publishes
  `ext_dev_host` on every instance row. What remains is kdeskdash colouring those rows (korg #2365),
  with no kvscf work first.
- **Retire the registry password on cleo and kwork** — korg WI 2404 (k-homelab's Windows changeover),
  after sprint 022. cleo needs only the per-host file; **kwork needs `KVSCF_REDIS_AUTH_KEY` in its
  `.env` before its registry value goes**, or it loses its dashboard. kwork is Ken's, at kwork.
- **`KVSCF_TOKEN`** stays on HKCU until korg WI 2479 decides whose secret it is.

## Later / Ideas

- Revisit `just` needing Git Bash's `sh` on Windows. A kprojects-wide question (every seeded
  justfile has it), not a kvscf one — raise it there rather than forking this repo's shell.
- Remaining threads from `sprints/review/2026-07-20-review.md` that were filed but not taken.

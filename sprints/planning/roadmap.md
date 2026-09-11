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
- **Tell the dashboard about dev hosts.** Sprint 021 flags them in the rail, but `ext_dev_host` is
  not on the `kvscf:instances:<host>` wire, so the kdeskdash panel still renders one as an ordinary
  window. Needs a wire-contract change and a kdeskdash-side change together.
- **First live AUTH run — premise moved, needs a status check.** Sprint 018 gave the publisher
  `KVSCF_REDIS_PASSWORD`. This item expected the first real use on **rpidash3**, via slice 5 of korg
  program 1143; in the event slice **korg:2231** put a `requirepass` on **rpidash2** on 2026-09-10
  and migrated cleo's kvscf to it the same night. So the credential is in use now, and the open
  question is no longer "when" but "did it authenticate" — which is answered from the Redis side,
  not from this repo.

## Later / Ideas

- Revisit `just` needing Git Bash's `sh` on Windows. A kprojects-wide question (every seeded
  justfile has it), not a kvscf one — raise it there rather than forking this repo's shell.
- Remaining threads from `sprints/review/2026-07-20-review.md` that were filed but not taken.

# Adapting kvscf's Code-tab title parser to your `window.title`

kvscf figures out a VS Code window's **workspace** and **remote host** from its **title bar text**.
The parser is tuned for a *folder-first* title (see the README's "Code tab: window titles"). If you'd
rather keep your own `window.title` than change it, this walkthrough shows how to adapt the parser.

It's written so **any coding agent — including a free tier — can do it**: the parser is one small,
pure Rust function with unit tests, no UI or Windows APIs involved.

## First: do you even need to change code?

Usually not. If you can set `window.title` so the **folder name comes first**, that's the whole fix —
see the README. Change code only if you want to keep a different title layout.

## What the parser expects

File: [`crates/kvscf-core/src/parse.rs`](../crates/kvscf-core/src/parse.rs), function `parse_title`.

Given a title, it does, in order:

1. Strip any stray `${…}` tokens (config drift).
2. Strip a leading "dirty" indicator (`●` / `•` / `*`) and an optional leading `- `.
3. Cut the app-name suffix at the **last** `" - Visual Studio Code"` (drops `… - Visual Studio Code
   - Insiders`, profile names, etc.).
4. Split what's left on the **first** `" - "`: the left side is the **rootName**, the right side is
   the active-file label.
5. From the rootName, peel a trailing `[SSH: host]` / `[WSL: distro]` / `[Dev Container: name]` /
   `[Codespaces]` bracket into the remote.

So the built-in assumption is: **`rootName` (folder, plus its `[SSH: host]` bracket if remote) is the
first `" - "`-separated segment; the active file, if any, follows.**

## Step 1 — capture your real titles

Build the core CLI and dump what your open windows actually report — you're adapting to *these*
strings, not a guess:

```sh
cargo run -p kvscf-core --bin kvscf-core -- list
```

Note where the **folder name**, the **`[SSH: host]`** (if you use remotes), and the **active file**
sit in each title, and what separates them.

## Step 2 — write the mapping as tests first

Add a couple of cases to the `#[cfg(test)] mod tests` block at the bottom of `parse.rs`, using **your**
titles and the results you expect. For example, if your title is
`main.rs — myproj — VS Code` (active file first, ` — ` em-dash separators):

```rust
#[test]
fn my_layout() {
    let r = p("main.rs — myproj — VS Code");
    assert_eq!(r.workspace, "myproj");
    assert_eq!(r.remote, Remote::Local);
    assert_eq!(r.active_file.as_deref(), Some("main.rs"));
}
```

Run them (they'll fail until step 3): `cargo test -p kvscf-core`.

## Step 3 — adjust `parse_title` to match

Change only what your layout needs. Common tweaks:

- **Different separator** (e.g. ` — ` em-dash, or `|`): change the `" - "` used in step 4's split and,
  if your app-name marker differs, `APP_MARKER`.
- **Active file *before* folder** (VS Code's default order): after the app-name cut, split on the
  **last** `" - "` instead of the first — the right side becomes the rootName, the left the active
  file. (Watch out if your folder names can contain the separator.)
- **A different remote indicator**: adjust `parse_root` — it looks for a trailing `[KIND: value]`.
- **No app-name suffix at all**: harmless — step 3 just finds nothing to cut and continues.

Keep the function **pure** (title in, `ParsedTitle` out) so the tests stay fast and portable — they run
on any OS, no VS Code or Windows needed.

## Step 4 — verify

```sh
cargo test -p kvscf-core          # your new cases pass, the existing ones still pass
cargo run -p kvscf-core --bin kvscf-core -- list   # labels look right against live windows
```

Then build and run the app (`cargo build --release -p kvscf-local`) and confirm the Code tab shows
your projects, not filenames.

## Don't touch

The Edge parser (`parse_edge_title`) and everything outside `parse.rs` — enumeration, focus, the UI —
are independent of `window.title`. This change is self-contained to `parse_title` and its tests.

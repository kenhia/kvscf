//! Tiny CLI over kvscf-core to exercise the walking skeleton.
//!
//!   kvscf-core            # same as `list`
//!   kvscf-core list       # print the sorted VS Code instance list (with hwnds)
//!   kvscf-core edge       # print open Edge windows (named first, then unnamed)
//!   kvscf-core focus <hwnd>

use kvscf_core::{find_app_window, focus, scan, scan_edge, AppMatcher};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        None | Some("list") => cmd_list(),
        Some("edge") => cmd_edge(),
        Some("find") => cmd_find(&args[2..]),
        Some("focus") => {
            let Some(hwnd) = args.get(2).and_then(|s| s.parse::<i64>().ok()) else {
                eprintln!("usage: kvscf-core focus <hwnd>");
                std::process::exit(2);
            };
            let ok = focus(hwnd);
            println!("focus({hwnd}) -> SetForegroundWindow returned {ok}");
        }
        Some(other) => {
            eprintln!(
                "unknown command: {other}\n\
                 usage: kvscf-core [list | edge | find [proc=X] [class=Y] [title=Z] | focus <hwnd>]"
            );
            std::process::exit(2);
        }
    }
}

fn cmd_find(args: &[String]) {
    let mut m = AppMatcher::default();
    for a in args {
        if let Some(v) = a.strip_prefix("proc=") {
            m.process = Some(v.to_string());
        } else if let Some(v) = a.strip_prefix("class=") {
            m.class = Some(v.to_string());
        } else if let Some(v) = a.strip_prefix("title=") {
            m.title_contains = Some(v.to_string());
        }
    }
    match find_app_window(&m) {
        Some(hwnd) => println!("found hwnd={hwnd}  (matcher: {m:?})"),
        None => println!("no window matched {m:?}"),
    }
}

fn cmd_edge() {
    let mut wins = scan_edge();
    // Named first (by name), then unnamed (by label) — the target UX.
    wins.sort_by(|a, b| {
        b.named
            .cmp(&a.named)
            .then_with(|| a.label.to_lowercase().cmp(&b.label.to_lowercase()))
    });
    if wins.is_empty() {
        println!("(no Edge windows open)");
        return;
    }
    for w in &wins {
        let kind = if w.named { "named" } else { "tab" };
        let tabs = w.tab_count.map(|n| format!("[{n}]")).unwrap_or_default();
        println!("{:>12}  {:<6} {:<4}  {}", w.hwnd, kind, tabs, w.label);
    }
    println!("\n{} Edge window(s).", wins.len());
}

fn cmd_list() {
    let mut items = scan();
    // Default sort: locals first, then by host, then workspace (case-insensitive).
    items.sort_by(|a, b| {
        let ha = a.remote.host().unwrap_or("");
        let hb = b.remote.host().unwrap_or("");
        ha.cmp(hb)
            .then_with(|| a.workspace.to_lowercase().cmp(&b.workspace.to_lowercase()))
    });

    if items.is_empty() {
        println!("(no VS Code windows open)");
        return;
    }

    for it in &items {
        let active = it.active_file.as_deref().unwrap_or("");
        println!(
            "{:>12}  {:<11}  {:<28}  {}",
            it.hwnd,
            it.app.label(),
            it.label(),
            active
        );
    }
    println!("\n{} window(s).", items.len());
}

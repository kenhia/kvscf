//! THROWAWAY SPIKE — korg kvscf #1132 / program 1143 slice 1.
//!
//! Question: does `SetForegroundWindow(target)` -> settle -> spawn `msedge.exe <url>`
//! actually land the new tab in *that* Edge window?
//!
//! Not part of the shipped surface. Delete once #1132 is answered, or keep as a
//! characterization tool. Nothing here is wired into the app.
//!
//! Usage:
//!   spike_url --list                 # enumerate Edge windows, change nothing
//!   spike_url --run                  # run the trial matrix (STEALS FOCUS, OPENS TABS)

use std::process::Command;
use std::thread::sleep;
use std::time::Duration;

use kvscf_core::{focus_with, foreground_hwnd, scan_edge, EdgeWindow};

/// Never touch this window — Ken is watching a movie on it.
const NEVER: &[&str] = &["Wowhead-Main"];

/// (target window name, settle delay in ms) — the trial matrix.
const TRIALS: &[(&str, u64)] = &[
    ("GitHub", 250),
    ("Homelab", 250),
    ("GitHub", 0),
    ("GitHub", 50),
    ("GitHub", 150),
    ("Homelab", 0),
];

/// How long to wait after spawning Edge before observing the result.
const OBSERVE_MS: u64 = 2500;

fn edge_exe() -> Option<String> {
    let candidates = [
        r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
    ];
    for c in candidates {
        if std::path::Path::new(c).exists() {
            return Some(c.to_string());
        }
    }
    None
}

fn snapshot() -> Vec<EdgeWindow> {
    let mut w = scan_edge();
    w.sort_by_key(|x| x.hwnd);
    w
}

fn find_named(list: &[EdgeWindow], name: &str) -> Option<EdgeWindow> {
    list.iter()
        .find(|w| w.named && w.label.eq_ignore_ascii_case(name))
        .cloned()
}

fn print_list(list: &[EdgeWindow]) {
    println!(
        "{:>12}  {:>5}  {:>3}  {:>4}  label",
        "HWND", "named", "z", "tabs"
    );
    let mut sorted = list.to_vec();
    sorted.sort_by_key(|w| w.z_index);
    for w in &sorted {
        println!(
            "{:>12}  {:>5}  {:>3}  {:>4}  {}",
            w.hwnd,
            w.named,
            w.z_index,
            w.tab_count
                .map(|t| t.to_string())
                .unwrap_or_else(|| "-".into()),
            w.label
        );
    }
}

fn find_any(list: &[EdgeWindow], name: &str) -> Option<EdgeWindow> {
    list.iter()
        .find(|w| w.label.eq_ignore_ascii_case(name))
        .cloned()
}

/// Trial A: target an *unnamed* window. Its kvscf label IS its active tab title, so if the
/// tab really lands there the label flips to the example.com page title. That converts the
/// named-window evidence (indirect: foreground + nothing else moved) into direct proof.
///
/// Trial B: minimize the target first. `focus_with` is supposed to SW_RESTORE it; this is a
/// real daily case (the window you want is minimized) and the one most likely to race.
fn confirm(exe: &str) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_MINIMIZE};

    // ---- Trial A: direct proof via an unnamed window's title ----
    println!("--- confirm A: unnamed target (title change = direct proof) ---");
    let before = snapshot();
    let unnamed = ["localhost", "korg"]
        .iter()
        .find_map(|n| find_any(&before, n).filter(|w| !w.named))
        .or_else(|| {
            before
                .iter()
                .find(|w| !w.named && !NEVER.iter().any(|n| w.label.eq_ignore_ascii_case(n)))
                .cloned()
        });

    match unnamed {
        None => println!("  SKIP: no unnamed Edge window available"),
        Some(t) => {
            println!("  target hwnd={} label={:?}", t.hwnd, t.label);
            focus_with(t.hwnd, false);
            sleep(Duration::from_millis(150));
            let _ = Command::new(exe)
                .arg("https://example.com/?kvscf-spike=confirm-a")
                .spawn();
            sleep(Duration::from_millis(OBSERVE_MS));

            let after = snapshot();
            let now = after.iter().find(|w| w.hwnd == t.hwnd);
            let fg = foreground_hwnd();
            match now {
                Some(w) => {
                    let changed = w.label != t.label;
                    println!("  label: {:?} -> {:?}", t.label, w.label);
                    println!("  foreground: {fg:?} (target {})", t.hwnd);
                    println!(
                        "  => {}",
                        if changed && fg == Some(t.hwnd) {
                            "DIRECT PASS — the tab is provably in the targeted window"
                        } else if fg == Some(t.hwnd) {
                            "INCONCLUSIVE — foreground right, title did not change"
                        } else {
                            "FAIL — tab went elsewhere"
                        }
                    );
                }
                None => println!("  target window vanished mid-trial"),
            }
        }
    }
    println!();

    // ---- Trial B: minimized target ----
    println!("--- confirm B: minimized target (restore + land) ---");
    let before = snapshot();
    match find_named(&before, "GitHub") {
        None => println!("  SKIP: no GitHub window"),
        Some(t) => {
            println!("  minimizing hwnd={} …", t.hwnd);
            unsafe {
                let _ = ShowWindow(HWND(t.hwnd as _), SW_MINIMIZE);
            }
            sleep(Duration::from_millis(700));

            let ok = focus_with(t.hwnd, false);
            sleep(Duration::from_millis(150));
            let fg_pre = foreground_hwnd();
            let _ = Command::new(exe)
                .arg("https://example.com/?kvscf-spike=confirm-b")
                .spawn();
            sleep(Duration::from_millis(OBSERVE_MS));

            let after = snapshot();
            let fg = foreground_hwnd();
            let new = after
                .iter()
                .filter(|w| !before.iter().any(|b| b.hwnd == w.hwnd))
                .count();
            println!("  focus_with -> {ok}; foreground before spawn {fg_pre:?}");
            println!("  foreground after: {fg:?}; new windows: {new}");
            println!(
                "  => {}",
                if fg == Some(t.hwnd) && new == 0 {
                    "PASS — restored from minimized and the tab landed there"
                } else {
                    "FAIL"
                }
            );
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let run = args.iter().any(|a| a == "--run");
    let do_confirm = args.iter().any(|a| a == "--confirm");

    let before_all = snapshot();
    println!("=== Edge windows on this box ({}) ===", before_all.len());
    print_list(&before_all);
    println!();

    if !run && !do_confirm {
        println!("(--list mode: nothing touched. Pass --run or --confirm to execute.)");
        return;
    }

    let Some(exe) = edge_exe() else {
        eprintln!("FATAL: could not locate msedge.exe in the standard install dirs");
        std::process::exit(1);
    };
    println!("edge: {exe}\n");

    if do_confirm {
        confirm(&exe);
        return;
    }

    // Refuse to proceed if a protected window would be a target.
    for (name, _) in TRIALS {
        if NEVER.iter().any(|n| n.eq_ignore_ascii_case(name)) {
            eprintln!("FATAL: trial matrix targets a protected window ({name})");
            std::process::exit(1);
        }
    }

    let mut results: Vec<String> = Vec::new();

    for (i, (name, delay)) in TRIALS.iter().enumerate() {
        let url = format!("https://example.com/?kvscf-spike={}-{}", i, delay);
        println!("--- trial {i}: target={name:?} delay={delay}ms ---");

        let before = snapshot();
        let Some(target) = find_named(&before, name) else {
            println!("  SKIP: no named window {name:?} is open");
            results.push(format!("{i}\t{name}\t{delay}\tSKIP(no such window)"));
            continue;
        };
        if NEVER.iter().any(|n| target.label.eq_ignore_ascii_case(n)) {
            println!("  SKIP: protected window");
            continue;
        }
        println!("  target hwnd={} z={}", target.hwnd, target.z_index);

        // 1. focus, 2. settle
        let focus_ok = focus_with(target.hwnd, false);
        sleep(Duration::from_millis(*delay));

        // Control: did focus actually take? If not, the trial says nothing about the
        // URL mechanism — it says the focus failed, which is a different bug.
        let fg_before_spawn = foreground_hwnd();
        let focus_landed = fg_before_spawn == Some(target.hwnd);
        println!(
            "  focus_with -> {focus_ok}; foreground now {:?} ({})",
            fg_before_spawn,
            if focus_landed {
                "ON TARGET"
            } else {
                "NOT target"
            }
        );

        // 3. spawn Edge with the URL
        match Command::new(&exe).arg(&url).spawn() {
            Ok(_) => {}
            Err(e) => {
                println!("  FATAL spawn error: {e}");
                results.push(format!("{i}\t{name}\t{delay}\tSPAWN-ERR"));
                continue;
            }
        }
        sleep(Duration::from_millis(OBSERVE_MS));

        // 4. observe
        let after = snapshot();
        let fg_after = foreground_hwnd();
        let landed_on_target = fg_after == Some(target.hwnd);

        let new_windows: Vec<&EdgeWindow> = after
            .iter()
            .filter(|w| !before.iter().any(|b| b.hwnd == w.hwnd))
            .collect();

        // An unnamed window's label IS its active tab title, so a window that grabbed
        // the tab shows up as a label change to the example.com page title.
        let title_changes: Vec<String> = after
            .iter()
            .filter_map(|w| {
                let old = before.iter().find(|b| b.hwnd == w.hwnd)?;
                (old.label != w.label)
                    .then(|| format!("{} : {:?} -> {:?}", w.hwnd, old.label, w.label))
            })
            .collect();

        let verdict = if !new_windows.is_empty() {
            "FAIL(new window)"
        } else if landed_on_target {
            "PASS"
        } else {
            "FAIL(other window)"
        };

        println!("  foreground after: {fg_after:?}");
        println!("  new windows: {}", new_windows.len());
        for w in &new_windows {
            println!("    + {} {:?}", w.hwnd, w.label);
        }
        println!("  title changes: {}", title_changes.len());
        for t in &title_changes {
            println!("    ~ {t}");
        }
        println!("  => {verdict}");
        println!();

        results.push(format!(
            "{i}\t{name}\t{delay}\t{verdict}\tfocus_landed={focus_landed}\tnew={}\tchanged={}",
            new_windows.len(),
            title_changes.len()
        ));

        sleep(Duration::from_millis(600));
    }

    println!("=== SUMMARY ===");
    println!("trial\ttarget\tdelay\tverdict\tnotes");
    for r in &results {
        println!("{r}");
    }
    println!("\n=== Edge windows after ===");
    print_list(&snapshot());
}

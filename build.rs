// FR-10 (read only): the application must never spawn an external process.
// The check lives in the build so a violation cannot reach a binary.

use std::fs;
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=src");
    let mut offenders = Vec::new();
    scan(Path::new("src"), &mut offenders);
    if !offenders.is_empty() {
        for o in &offenders {
            println!("cargo:warning=FR-10 violation: {o}");
        }
        panic!(
            "FR-10: process-spawning calls found in {} place(s); the application is read only",
            offenders.len()
        );
    }
}

fn scan(dir: &Path, out: &mut Vec<String>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan(&path, out);
        } else if path.extension().map(|e| e == "rs").unwrap_or(false) {
            let text = match fs::read_to_string(&path) {
                Ok(t) => t,
                Err(_) => continue,
            };
            for (i, line) in text.lines().enumerate() {
                let code = line.split("//").next().unwrap_or("");
                for needle in ["process::Command", "Command::new", "std::process::exit("] {
                    if needle == "std::process::exit(" {
                        continue; // exiting is not spawning
                    }
                    if code.contains(needle) {
                        out.push(format!("{}:{}: {}", path.display(), i + 1, line.trim()));
                    }
                }
            }
        }
    }
}

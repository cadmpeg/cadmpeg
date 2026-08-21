// SPDX-License-Identifier: Apache-2.0
//! Embeds the git revision so command reports can name the binary that
//! wrote them (stale-artifact detection). Falls back to "unknown" when
//! git or the repository is unavailable (for example a crates.io build).

fn git(args: &[&str]) -> Option<String> {
    std::process::Command::new("git")
        .args(args)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
}

fn existing_git_path(relative: &str) -> Option<String> {
    let path = git(&["rev-parse", "--git-path", relative])?;
    std::path::Path::new(&path).exists().then_some(path)
}

fn stamp_inputs() -> Vec<String> {
    let head = existing_git_path("HEAD");
    let reference = match git(&["symbolic-ref", "-q", "HEAD"]) {
        Some(reference) => {
            existing_git_path(&reference).or_else(|| existing_git_path("packed-refs"))
        }
        None => None,
    };
    head.into_iter().chain(reference).collect()
}

fn main() {
    let revision = git(&["rev-parse", "--short=12", "HEAD"]);
    if revision.is_some() {
        for path in stamp_inputs() {
            println!("cargo:rerun-if-changed={path}");
        }
    }
    println!(
        "cargo:rustc-env=CADMPEG_BUILD_GIT={}",
        revision.as_deref().unwrap_or("unknown")
    );
}

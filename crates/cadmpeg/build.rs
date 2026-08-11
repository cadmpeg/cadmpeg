// SPDX-License-Identifier: Apache-2.0
//! Embeds the git revision so command reports can name the binary that
//! wrote them (stale-artifact detection). Falls back to "unknown" when
//! git or the repository is unavailable (for example a crates.io build).

fn main() {
    let hash = std::process::Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "unknown".to_owned());
    println!("cargo:rustc-env=CADMPEG_BUILD_GIT={hash}");
}

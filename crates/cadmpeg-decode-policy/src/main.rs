// SPDX-License-Identifier: Apache-2.0
#![feature(rustc_private)]
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut arguments: Vec<String> = std::env::args().collect();
    if arguments.get(1).is_some_and(|argument| argument.ends_with("rustc")) { arguments.remove(1); }
    if cadmpeg_decode_policy::run(&arguments) && std::env::var_os("CADMPEG_POLICY_COLLECT").is_none() { ExitCode::FAILURE } else { ExitCode::SUCCESS }
}

// SPDX-License-Identifier: Apache-2.0
//! The `cadmpeg` binary under test, shared by the CLI-driving integration
//! test binaries in this directory.

use assert_cmd::Command;

/// The built `cadmpeg` binary, ready to take arguments.
pub fn cadmpeg() -> Command {
    Command::cargo_bin("cadmpeg").unwrap()
}

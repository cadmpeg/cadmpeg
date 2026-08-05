// SPDX-License-Identifier: Apache-2.0
//! Identity of the format that owns entity IDs decoded from an ASM stream.

/// Format component of entity IDs emitted from one ASM stream, for example
/// `f3d` in `f3d:brep:entity#5`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdFormat<'a>(pub &'a str);

impl std::fmt::Display for IdFormat<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

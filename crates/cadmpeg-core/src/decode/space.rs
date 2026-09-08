// SPDX-License-Identifier: Apache-2.0
//! Address-space identifiers and stable descriptors.
//!
//! Every byte a decode reads belongs to exactly one address space: the root
//! input, an inflated entry, a reconstructed stream. A [`SpaceId`] names one
//! space within a single decode session; a [`View`](crate::decode::View)
//! carries the id so error locations fall out of the type. Coordinates are
//! absolute within a space: offset zero is that space's first byte.
//!
//! A [`SpaceId`] is session-local. [`SpaceDescriptor`] records the stable
//! label, parent, and derivation needed to resolve a location into a
//! root-to-leaf [`ResolvedAddress`] before the decode context is dropped.

/// Names one address space within a single decode.
///
/// Ids are dense and assigned in registration order; the root is always
/// [`SpaceId::ROOT`]. Error locations use the id to qualify offsets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SpaceId(usize);

impl SpaceId {
    /// The root input space, registered first by every decode.
    pub const ROOT: SpaceId = SpaceId(0);

    /// Creates a session-local address-space identifier.
    pub(crate) const fn from_index(index: usize) -> Self {
        Self(index)
    }

    /// Returns the dense index of this space.
    pub fn index(self) -> usize {
        self.0
    }
}

/// A half-open byte range `[start, end)` within one space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteRange {
    /// Inclusive start offset.
    pub start: u64,
    /// Exclusive end offset.
    pub end: u64,
}

/// How a registered space was derived from its parent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpaceDerivation {
    /// The root input image.
    Root,
    /// A stored (uncompressed) child range borrowed from the parent.
    StoredSlice {
        /// Parent space.
        parent: SpaceId,
        /// Range in the parent space.
        range: ByteRange,
    },
    /// Decompressed or otherwise expanded output from a parent range.
    Expanded {
        /// Parent space that supplied the compressed bytes.
        parent: SpaceId,
        /// Compressed source range in the parent space.
        source_range: ByteRange,
    },
    /// Concatenation of several parent windows.
    Concatenated {
        /// First parent space.
        first_parent: SpaceId,
        /// Remaining parent spaces, in concatenation order.
        additional_parents: Vec<SpaceId>,
    },
}

/// Stable description of one registered address space.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpaceDescriptor {
    /// Stable label (archive member name, stream path, or `"root"`).
    pub label: String,
    /// How this space was derived.
    pub derivation: SpaceDerivation,
}

/// One step in a root-to-leaf resolved address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddressStep {
    /// Stable label for this step.
    pub label: String,
    /// Whether this step is the root or an archive member.
    pub kind: AddressStepKind,
}

/// Kind of one address step, for inspect replay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressStepKind {
    /// Root input file.
    Root,
    /// Archive member, stored or expanded.
    Member,
}

/// Owned root-to-leaf address that survives the decode session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedAddress {
    /// Path from root to the space that owns the offset.
    pub steps: Vec<AddressStep>,
    /// Absolute offset within the leaf space.
    pub offset: u64,
}

impl ResolvedAddress {
    /// Formats the address as a stable path string.
    pub fn path(&self) -> String {
        let mut out = String::new();
        for (index, step) in self.steps.iter().enumerate() {
            if index > 0 {
                out.push('/');
            }
            out.push_str(&step.label);
        }
        out.push('@');
        out.push_str(&self.offset.to_string());
        out
    }

    /// Returns POSIX shell commands that replay this byte location with `cadmpeg inspect`.
    ///
    /// Nested archive members emit `inspect extract` then `inspect hex` on the
    /// extracted member. Root-only addresses emit `inspect hex` on the file.
    pub fn inspect_commands(&self, file: &str) -> Vec<String> {
        let quote = |value: &str| format!("'{}'", value.replace('\'', "'\\''"));
        let mut input = file.to_owned();
        let mut commands = Vec::new();
        for step in self
            .steps
            .iter()
            .filter(|step| step.kind == AddressStepKind::Member)
        {
            let extracted = format!("{input}.member");
            commands.push(format!(
                "cadmpeg inspect extract --output={} -- {} {}",
                quote(&extracted),
                quote(&input),
                quote(&step.label),
            ));
            input = extracted;
        }
        commands.push(format!(
            "cadmpeg inspect hex --offset {} --len 64 -- {}",
            self.offset,
            quote(&input),
        ));
        commands
    }
}

/// Resolves a session-local location against a descriptor table.
pub fn resolve_address(
    descriptors: &[SpaceDescriptor],
    location: super::error::SourceLocation,
) -> ResolvedAddress {
    let mut steps = Vec::new();
    let mut current = location.space;
    for _ in 0..=descriptors.len() {
        let Some(descriptor) = descriptors.get(current.index()) else {
            break;
        };
        let kind = match descriptor.derivation {
            SpaceDerivation::Root => AddressStepKind::Root,
            SpaceDerivation::StoredSlice { .. }
            | SpaceDerivation::Expanded { .. }
            | SpaceDerivation::Concatenated { .. } => AddressStepKind::Member,
        };
        steps.push(AddressStep {
            label: descriptor.label.clone(),
            kind,
        });
        match descriptor.derivation {
            SpaceDerivation::Root => break,
            SpaceDerivation::StoredSlice { parent, .. }
            | SpaceDerivation::Expanded { parent, .. } => {
                current = parent;
            }
            SpaceDerivation::Concatenated { first_parent, .. } => current = first_parent,
        }
    }
    steps.reverse();
    ResolvedAddress {
        steps,
        offset: location.offset,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::SourceLocation;

    #[test]
    fn nested_members_emit_every_extraction() {
        let descriptors = [
            SpaceDescriptor {
                label: "root".into(),
                derivation: SpaceDerivation::Root,
            },
            SpaceDescriptor {
                label: "Assets/inner archive.zip".into(),
                derivation: SpaceDerivation::Expanded {
                    parent: SpaceId::ROOT,
                    source_range: ByteRange { start: 30, end: 90 },
                },
            },
            SpaceDescriptor {
                label: "Data/payload bytes.bin".into(),
                derivation: SpaceDerivation::StoredSlice {
                    parent: SpaceId::from_index(1),
                    range: ByteRange { start: 20, end: 84 },
                },
            },
        ];
        let address = resolve_address(
            &descriptors,
            SourceLocation {
                space: SpaceId::from_index(2),
                offset: 7,
            },
        );
        assert_eq!(address.inspect_commands("project part.FCStd"), [
            "cadmpeg inspect extract --output='project part.FCStd.member' -- 'project part.FCStd' 'Assets/inner archive.zip'",
            "cadmpeg inspect extract --output='project part.FCStd.member.member' -- 'project part.FCStd.member' 'Data/payload bytes.bin'",
            "cadmpeg inspect hex --offset 7 --len 64 -- 'project part.FCStd.member.member'",
        ]);
    }

    #[cfg(unix)]
    #[test]
    fn replay_paths_survive_posix_shell_parsing() {
        let file = "-project's $HOME;*.FCStd";
        let address = ResolvedAddress {
            steps: Vec::new(),
            offset: 7,
        };
        let commands = address.inspect_commands(file);
        let output = std::process::Command::new("sh")
            .arg("-c")
            .arg(format!(
                "cadmpeg() {{ printf '%s\\0' \"$@\"; }}; {}",
                commands[0]
            ))
            .output()
            .expect("POSIX shell captures replay arguments");
        assert!(output.status.success());
        let arguments = output
            .stdout
            .split(|byte| *byte == 0)
            .filter(|value| !value.is_empty())
            .map(|value| std::str::from_utf8(value).expect("replay argument is UTF-8"))
            .collect::<Vec<_>>();
        assert_eq!(
            arguments,
            ["inspect", "hex", "--offset", "7", "--len", "64", "--", file]
        );
    }
}

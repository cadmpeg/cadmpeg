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
    /// Whether this step is the root, a stored member, or an expansion.
    pub kind: AddressStepKind,
}

/// Kind of one address step, for inspect replay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressStepKind {
    /// Root input file.
    Root,
    /// Stored archive member (borrowed parent bytes).
    StoredMember,
    /// Expanded (inflated) archive member.
    ExpandedMember,
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

    /// Returns `cadmpeg inspect` commands that replay this byte location.
    ///
    /// Nested archive members emit `inspect extract` then `inspect hex` on the
    /// extracted member. Root-only addresses emit `inspect hex` on the file.
    pub fn inspect_commands(&self, file: &str) -> Vec<String> {
        let leaf = self.steps.last();
        match leaf {
            None
            | Some(AddressStep {
                kind: AddressStepKind::Root,
                ..
            }) => {
                vec![format!(
                    "cadmpeg inspect hex {file} --offset {} --len 64",
                    self.offset
                )]
            }
            Some(AddressStep {
                kind: AddressStepKind::StoredMember | AddressStepKind::ExpandedMember,
                label: member,
            }) => {
                let extracted = format!("{file}.member");
                vec![
                    format!("cadmpeg inspect extract {file} {member} -o {extracted}"),
                    format!(
                        "cadmpeg inspect hex {extracted} --offset {} --len 64",
                        self.offset
                    ),
                ]
            }
        }
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
            SpaceDerivation::StoredSlice { .. } => AddressStepKind::StoredMember,
            SpaceDerivation::Expanded { .. } | SpaceDerivation::Concatenated { .. } => {
                AddressStepKind::ExpandedMember
            }
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

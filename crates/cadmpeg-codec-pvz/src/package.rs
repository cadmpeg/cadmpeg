// SPDX-License-Identifier: Apache-2.0
//! The `.pvz` ZIP package: member roles and the one structure member.

use std::collections::BTreeSet;

use cadmpeg_container::ArchiveSnapshot;
use cadmpeg_core::container::ContainerRole;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::{CodecError, ContainerEntry};
use cadmpeg_ir::report::loss::LossNote;

use crate::loss::PvzLossCode;

/// Member-name extension of the structure member.
pub(crate) const STRUCTURE_EXTENSION: &str = ".pvs";
/// Member-name extension of a geometry member.
const GEOMETRY_EXTENSION: &str = ".ol";

/// The package role a member name selects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MemberRole {
    /// The `.pvs` structure member.
    Structure,
    /// An `.ol` geometry member.
    Geometry,
    /// Any other member.
    Other,
}

impl MemberRole {
    /// The role of the member called `name`. The extension comparison
    /// ignores ASCII case.
    pub(crate) fn of(name: &str) -> Self {
        if has_extension(name, STRUCTURE_EXTENSION) {
            Self::Structure
        } else if has_extension(name, GEOMETRY_EXTENSION) {
            Self::Geometry
        } else {
            Self::Other
        }
    }

    const fn container_role(self) -> ContainerRole {
        match self {
            Self::Structure => ContainerRole::Document,
            Self::Geometry => ContainerRole::ModelData,
            Self::Other => ContainerRole::Other,
        }
    }
}

/// Whether `name` is a structure-member name.
pub(crate) fn is_structure_name(name: &str) -> bool {
    MemberRole::of(name) == MemberRole::Structure
}

/// Whether `name` has a nonempty stem followed by `extension`.
fn has_extension(name: &str, extension: &str) -> bool {
    name.len()
        .checked_sub(extension.len())
        .filter(|stem| *stem > 0)
        .and_then(|stem| name.get(stem..))
        .is_some_and(|tail| tail.eq_ignore_ascii_case(extension))
}

/// An admitted package: the archive and its member partition.
pub(crate) struct Package<'a> {
    archive: ArchiveSnapshot<'a>,
    /// Names of every member.
    names: BTreeSet<String>,
    structure: String,
    geometry: Vec<String>,
}

impl<'a> Package<'a> {
    /// Opens the archive and locates its one structure member.
    pub(crate) fn open(ctx: &DecodeContext<'a>, root: View<'a>) -> Result<Self, CodecError> {
        let archive = ArchiveSnapshot::new(ctx, root)?;
        let mut structures = Vec::new();
        let mut geometry = Vec::new();
        let mut names = BTreeSet::new();
        for entry in archive.entries() {
            names.insert(entry.name.clone());
            match MemberRole::of(&entry.name) {
                MemberRole::Structure => structures.push(entry.name.clone()),
                MemberRole::Geometry => geometry.push(entry.name.clone()),
                MemberRole::Other => {}
            }
        }
        let mut structures = structures.into_iter();
        let Some(structure) = structures.next() else {
            return Err(CodecError::WrongFormat(
                "not a .pvz package: the archive has no .pvs structure member".into(),
            ));
        };
        if let Some(second) = structures.next() {
            return Err(CodecError::malformed(format_args!(
                ".pvz package has more than one .pvs structure member: {structure} and {second}"
            )));
        }
        Ok(Self {
            archive,
            names,
            structure,
            geometry,
        })
    }

    /// Name of the structure member.
    pub(crate) fn structure_name(&self) -> &str {
        &self.structure
    }

    /// Names of the geometry members, in archive order.
    pub(crate) fn geometry_names(&self) -> &[String] {
        &self.geometry
    }

    /// Opens the structure member.
    pub(crate) fn structure(&self, ctx: &DecodeContext<'a>) -> Result<View<'a>, CodecError> {
        self.archive.open(ctx, &self.structure)
    }

    /// Whether the archive holds a member named exactly `name`.
    pub(crate) fn contains(&self, name: &str) -> bool {
        self.names.contains(name)
    }

    /// Opens one member by its exact archive name.
    pub(crate) fn member(
        &self,
        ctx: &DecodeContext<'a>,
        name: &str,
    ) -> Result<View<'a>, CodecError> {
        self.archive.open(ctx, name)
    }

    /// Summary entries for every member, with their package roles.
    pub(crate) fn entries(&self) -> Vec<ContainerEntry> {
        self.archive
            .container_entries(|name| MemberRole::of(name).container_role())
    }

    /// The loss charged when the members came from local-header recovery.
    pub(crate) fn recovery_loss(&self) -> Option<LossNote> {
        self.archive.recovery_reason().map(|reason| {
            PvzLossCode::ContainerDirectoryRecovered.note(format!(
                "the ZIP central directory is unreadable ({reason}); members were recovered \
                 from their local headers"
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::MemberRole;

    #[test]
    fn member_roles_follow_the_name_extension() {
        for (name, role) in [
            ("model.pvs", MemberRole::Structure),
            ("dir/MODEL.PVS", MemberRole::Structure),
            ("part_1.ol", MemberRole::Geometry),
            ("dir/PART.OL", MemberRole::Geometry),
            (".pvs", MemberRole::Other),
            (".ol", MemberRole::Other),
            ("model.pvs.bak", MemberRole::Other),
            ("thumbnail.png", MemberRole::Other),
            ("", MemberRole::Other),
        ] {
            assert_eq!(MemberRole::of(name), role, "{name:?}");
        }
    }
}

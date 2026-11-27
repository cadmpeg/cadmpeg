// SPDX-License-Identifier: Apache-2.0
//! The eDrawings ZIP package: member roles and the one model member.

use cadmpeg_container::ArchiveSnapshot;
use cadmpeg_core::container::ContainerRole;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::{CodecError, ContainerEntry};
use cadmpeg_ir::report::loss::LossNote;

use crate::loss::EdrawingsLossCode;

/// Name of the model member.
pub(crate) const MODEL_MEMBER: &str = "eModel";
/// Name of the preview image member.
const PREVIEW_MEMBER: &str = "preview.jpg";
/// Name of the scene description member.
const SCENE_MEMBER: &str = "scene.xml";
/// Name of the material table member (assemblies).
const MATERIALS_MEMBER: &str = "materials.xml";
/// Name of the document property member (assemblies).
const PROPERTIES_MEMBER: &str = "FileProperties.xml";
/// Name of the small opaque member.
const OLE_MEMBER: &str = "OLE";

/// The package role a member name selects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MemberRole {
    /// The `eModel` member.
    Model,
    /// The `preview.jpg` member.
    Preview,
    /// The `scene.xml` member.
    Scene,
    /// The `materials.xml` member.
    Materials,
    /// The `FileProperties.xml` member.
    Properties,
    /// The `OLE` member.
    Ole,
    /// Any other member.
    Other,
}

impl MemberRole {
    /// The role of the member called `name`. Names compare exactly.
    pub(crate) fn of(name: &str) -> Self {
        match name {
            MODEL_MEMBER => Self::Model,
            PREVIEW_MEMBER => Self::Preview,
            SCENE_MEMBER => Self::Scene,
            MATERIALS_MEMBER => Self::Materials,
            PROPERTIES_MEMBER => Self::Properties,
            OLE_MEMBER => Self::Ole,
            _ => Self::Other,
        }
    }

    pub(crate) const fn container_role(self) -> ContainerRole {
        match self {
            Self::Model => ContainerRole::ModelData,
            Self::Preview => ContainerRole::PreviewImage,
            Self::Scene => ContainerRole::Auxiliary,
            Self::Materials => ContainerRole::MaterialTexture,
            Self::Properties => ContainerRole::Properties,
            Self::Ole => ContainerRole::Opaque,
            Self::Other => ContainerRole::Other,
        }
    }
}

/// Whether `name` is the model member name.
pub(crate) fn is_model_name(name: &str) -> bool {
    MemberRole::of(name) == MemberRole::Model
}

/// An admitted package: the archive and its model member.
pub(crate) struct Package<'a> {
    root: View<'a>,
    archive: ArchiveSnapshot<'a>,
}

impl<'a> Package<'a> {
    /// Opens the archive and requires its `eModel` member.
    pub(crate) fn open(ctx: &DecodeContext<'a>, root: View<'a>) -> Result<Self, CodecError> {
        let archive = ArchiveSnapshot::new(ctx, root)?;
        if archive.entry(MODEL_MEMBER).is_none() {
            return Err(CodecError::WrongFormat(
                "not an eDrawings package: the archive has no eModel member".into(),
            ));
        }
        Ok(Self { root, archive })
    }

    /// Opens the model member.
    pub(crate) fn model(&self, ctx: &DecodeContext<'a>) -> Result<View<'a>, CodecError> {
        self.archive.open(ctx, MODEL_MEMBER)
    }

    /// The stored bytes of the model member, as the archive frames them.
    pub(crate) fn stored_model(&self) -> Result<View<'a>, CodecError> {
        let entry = self
            .archive
            .entry(MODEL_MEMBER)
            .ok_or_else(|| CodecError::malformed("the archive has no eModel member"))?;
        let range = entry.stored_range()?;
        usize::try_from(range.start)
            .ok()
            .zip(usize::try_from(range.end).ok())
            .and_then(|(start, end)| View::over_retained(self.root.window()).child(start, end))
            .ok_or_else(|| CodecError::malformed("the eModel member escapes the package"))
    }

    /// Number of members.
    pub(crate) fn member_count(&self) -> usize {
        self.archive.entries().len()
    }

    /// Summary entries for every member, with their package roles.
    pub(crate) fn entries(&self) -> Vec<ContainerEntry> {
        self.archive
            .container_entries(|name| MemberRole::of(name).container_role())
    }

    /// The loss charged when the members came from local-header recovery.
    pub(crate) fn recovery_loss(&self) -> Option<LossNote> {
        self.archive.recovery_reason().map(|reason| {
            EdrawingsLossCode::ContainerDirectoryRecovered.note(format!(
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
    fn member_roles_follow_the_member_name() {
        for (name, role) in [
            ("eModel", MemberRole::Model),
            ("preview.jpg", MemberRole::Preview),
            ("scene.xml", MemberRole::Scene),
            ("materials.xml", MemberRole::Materials),
            ("FileProperties.xml", MemberRole::Properties),
            ("OLE", MemberRole::Ole),
            ("emodel", MemberRole::Other),
            ("dir/eModel", MemberRole::Other),
            ("", MemberRole::Other),
        ] {
            assert_eq!(MemberRole::of(name), role, "{name}");
        }
    }
}

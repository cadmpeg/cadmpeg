// SPDX-License-Identifier: Apache-2.0
//! The design container: a ZIP package of named parts.
//!
//! `container.json` holds the container marker and `document.json` the design
//! document. Payloads are content-addressed blobs `blobs/<sha256>.<ext>`,
//! hashed over their exact expanded bytes. `cache/` holds disposable caches
//! keyed to the document, `plugins/` the data of application plugins, and
//! `provenance/` the signed save log.

use std::collections::BTreeMap;

use cadmpeg_container::ArchiveSnapshot;
use cadmpeg_core::container::ContainerRole;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::{CodecError, ContainerEntry};
use cadmpeg_ir::report::loss::LossNote;
use serde_json::Value;

use crate::loss::SolidcraftLossCode;

/// The container marker member.
pub(crate) const MARKER_MEMBER: &str = "container.json";
/// The design document member.
pub(crate) const DOCUMENT_MEMBER: &str = "document.json";
/// The drawing document member of a drawing container.
pub(crate) const DRAWING_MEMBER: &str = "drawing.json";
/// The marker format of version 1 containers.
pub(crate) const CONTAINER_FORMAT: &str = "solidcraft-container/1";
/// The marker format of draft version 1 containers.
pub(crate) const DRAFT_CONTAINER_FORMAT: &str = "solvecraft-container/1";
/// The marker `content` value of a drawing container.
const DRAWING_CONTENT: &str = "drawing";
/// The blob directory prefix.
const BLOB_PREFIX: &str = "blobs/";
/// Hexadecimal digits in a blob digest.
const BLOB_DIGEST_LEN: usize = 64;

/// The payload encoding a blob extension names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum BlobKind {
    /// STEP Part 21 text of an import feature.
    Step,
    /// `SCMESH` triangle mesh of a mesh import.
    Mesh,
    /// PNG canvas image.
    Png,
    /// JPEG canvas image.
    Jpeg,
    /// `SCPNTS` point cloud.
    Points,
    /// Radiance HDR environment image.
    Hdr,
    /// Opaque render texture.
    Texture,
}

impl BlobKind {
    fn of_extension(extension: &str) -> Option<Self> {
        Some(match extension {
            "step" => Self::Step,
            "mesh" => Self::Mesh,
            "png" => Self::Png,
            "jpeg" => Self::Jpeg,
            "points" => Self::Points,
            "hdr" => Self::Hdr,
            "texture" => Self::Texture,
            _ => return None,
        })
    }

    /// The name extension.
    pub(crate) const fn extension(self) -> &'static str {
        match self {
            Self::Step => "step",
            Self::Mesh => "mesh",
            Self::Png => "png",
            Self::Jpeg => "jpeg",
            Self::Points => "points",
            Self::Hdr => "hdr",
            Self::Texture => "texture",
        }
    }

    const fn container_role(self) -> ContainerRole {
        match self {
            Self::Step | Self::Mesh | Self::Points => ContainerRole::ModelData,
            Self::Png | Self::Jpeg => ContainerRole::Image,
            Self::Hdr | Self::Texture => ContainerRole::MaterialTexture,
        }
    }
}

/// A well-formed blob member name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct BlobName {
    /// The lowercase hexadecimal SHA-256 of the expanded payload.
    pub(crate) digest: String,
    /// The payload encoding.
    pub(crate) kind: BlobKind,
}

impl BlobName {
    /// Parses `blobs/<64 lowercase hex digits>.<known extension>`.
    pub(crate) fn parse(name: &str) -> Option<Self> {
        let (digest, extension) = name.strip_prefix(BLOB_PREFIX)?.split_once('.')?;
        let kind = BlobKind::of_extension(extension)?;
        (digest.len() == BLOB_DIGEST_LEN
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)))
        .then(|| Self {
            digest: digest.to_owned(),
            kind,
        })
    }
}

/// The package role a member name selects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MemberRole {
    /// `container.json`.
    Marker,
    /// `document.json`.
    Document,
    /// `drawing.json`.
    Drawing,
    /// A well-formed blob name.
    Blob(BlobName),
    /// `cache/model.json`.
    ModelManifest,
    /// `cache/bodies.bin`.
    ModelBodies,
    /// `cache/preview.json` or `cache/preview.bin`.
    Preview,
    /// `cache/thumbnail.png`.
    Thumbnail,
    /// `cache/thumbnail.json`.
    ThumbnailKey,
    /// Another `cache/` member.
    Cache,
    /// A `plugins/` member.
    Plugin,
    /// A `provenance/` member.
    Provenance,
    /// Any other member.
    Other,
}

impl MemberRole {
    /// The role of the member called `name`.
    pub(crate) fn of(name: &str) -> Self {
        match name {
            MARKER_MEMBER => Self::Marker,
            DOCUMENT_MEMBER => Self::Document,
            DRAWING_MEMBER => Self::Drawing,
            "cache/model.json" => Self::ModelManifest,
            "cache/bodies.bin" => Self::ModelBodies,
            "cache/preview.json" | "cache/preview.bin" => Self::Preview,
            "cache/thumbnail.png" => Self::Thumbnail,
            "cache/thumbnail.json" => Self::ThumbnailKey,
            _ => {
                if let Some(blob) = BlobName::parse(name) {
                    Self::Blob(blob)
                } else if name.starts_with("cache/") {
                    Self::Cache
                } else if name.starts_with("plugins/") {
                    Self::Plugin
                } else if name.starts_with("provenance/") {
                    Self::Provenance
                } else {
                    Self::Other
                }
            }
        }
    }

    const fn container_role(&self) -> ContainerRole {
        match self {
            Self::Marker => ContainerRole::Manifest,
            Self::Document | Self::Drawing => ContainerRole::Document,
            Self::Blob(blob) => blob.kind.container_role(),
            Self::ModelManifest | Self::Cache => ContainerRole::CacheCell,
            Self::ModelBodies => ContainerRole::Brep,
            Self::Preview => ContainerRole::Preview,
            Self::Thumbnail => ContainerRole::Thumbnail,
            Self::ThumbnailKey => ContainerRole::Metadata,
            Self::Plugin => ContainerRole::Opaque,
            Self::Provenance => ContainerRole::Signature,
            Self::Other => ContainerRole::Other,
        }
    }
}

/// What `container.json` declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Marker {
    /// Exactly `{"format":"solidcraft-container/1"}`.
    Design,
    /// Exactly `{"format":"solvecraft-container/1"}`.
    DraftDesign,
    /// A version 1 marker with `"content":"drawing"`.
    Drawing,
    /// Any other marker, or a marker that does not parse, as its text.
    Other(String),
    /// The package has no marker member.
    Absent,
}

impl Marker {
    fn of(text: &str, value: Option<&Value>) -> Self {
        let Some(Value::Object(object)) = value else {
            return Self::Other(text.to_owned());
        };
        let format = object.get("format").and_then(Value::as_str);
        let content = object.get("content");
        match (format, content, object.len()) {
            (Some(CONTAINER_FORMAT), None, 1) => Self::Design,
            (Some(DRAFT_CONTAINER_FORMAT), None, 1) => Self::DraftDesign,
            (Some(CONTAINER_FORMAT), Some(Value::String(content)), 2)
                if content == DRAWING_CONTENT =>
            {
                Self::Drawing
            }
            _ => Self::Other(text.to_owned()),
        }
    }

    /// The declared container format text.
    pub(crate) fn declared(&self) -> Option<&str> {
        match self {
            Self::Design | Self::Drawing => Some(CONTAINER_FORMAT),
            Self::DraftDesign => Some(DRAFT_CONTAINER_FORMAT),
            Self::Other(text) => Some(text),
            Self::Absent => None,
        }
    }
}

/// An admitted package: the archive and its member partition.
pub(crate) struct Package<'a> {
    root: View<'a>,
    archive: ArchiveSnapshot<'a>,
    roles: BTreeMap<String, MemberRole>,
    marker: Marker,
}

impl<'a> Package<'a> {
    /// Opens the archive, assigns member roles, and reads the marker.
    pub(crate) fn open(ctx: &DecodeContext<'a>, root: View<'a>) -> Result<Self, CodecError> {
        let archive = ArchiveSnapshot::new(ctx, root)?;
        let roles: BTreeMap<String, MemberRole> = archive
            .entries()
            .iter()
            .map(|entry| (entry.name.clone(), MemberRole::of(&entry.name)))
            .collect();
        if !roles.contains_key(MARKER_MEMBER)
            && !roles.contains_key(DOCUMENT_MEMBER)
            && !roles.contains_key(DRAWING_MEMBER)
        {
            return Err(CodecError::WrongFormat(
                "not a SolidCraft container: the archive has neither container.json nor \
                 document.json"
                    .into(),
            ));
        }
        let marker = if roles.contains_key(MARKER_MEMBER) {
            let view = archive.open(ctx, MARKER_MEMBER)?;
            let text = String::from_utf8_lossy(view.window()).into_owned();
            let value = std::str::from_utf8(view.window())
                .ok()
                .map(|text| ctx.parse_json_value(text, "SolidCraft container marker"))
                .transpose();
            let value = match value {
                Ok(value) => value,
                Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
                Err(_) => None,
            };
            Marker::of(&text, value.as_ref())
        } else {
            Marker::Absent
        };
        Ok(Self {
            root,
            archive,
            roles,
            marker,
        })
    }

    /// The container marker.
    pub(crate) fn marker(&self) -> &Marker {
        &self.marker
    }

    /// Every member name with its role, in name order.
    pub(crate) fn roles(&self) -> &BTreeMap<String, MemberRole> {
        &self.roles
    }

    /// Whether the archive holds a member named exactly `name`.
    pub(crate) fn contains(&self, name: &str) -> bool {
        self.roles.contains_key(name)
    }

    /// Opens one member by its exact archive name.
    pub(crate) fn member(
        &self,
        ctx: &DecodeContext<'a>,
        name: &str,
    ) -> Result<View<'a>, CodecError> {
        self.archive.open(ctx, name)
    }

    /// The stored bytes of member `name`, at their offsets in the package.
    pub(crate) fn stored(&self, name: &str) -> Result<View<'a>, CodecError> {
        let entry = self
            .archive
            .entry(name)
            .ok_or_else(|| CodecError::malformed(format_args!("no member named {name:?}")))?;
        let range = entry.stored_range()?;
        usize::try_from(range.start)
            .ok()
            .zip(usize::try_from(range.end).ok())
            .and_then(|(start, end)| View::over_retained(self.root.window()).child(start, end))
            .ok_or_else(|| {
                CodecError::malformed(format_args!("member {name:?} escapes the package"))
            })
    }

    /// Summary entries for every member, with their package roles.
    pub(crate) fn entries(&self) -> Vec<ContainerEntry> {
        self.archive
            .container_entries(|name| MemberRole::of(name).container_role())
    }

    /// The loss charged when the members came from local-header recovery.
    pub(crate) fn recovery_loss(&self) -> Option<LossNote> {
        self.archive.recovery_reason().map(|reason| {
            SolidcraftLossCode::ContainerDirectoryRecovered.note(format!(
                "the ZIP central directory is unreadable ({reason}); members were recovered \
                 from their local headers"
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{BlobKind, BlobName, Marker, MemberRole};

    #[test]
    fn blob_names_need_a_lowercase_digest_and_a_known_extension() {
        let digest = "0123456789abcdef".repeat(4);
        assert_eq!(
            BlobName::parse(&format!("blobs/{digest}.step")),
            Some(BlobName {
                digest: digest.clone(),
                kind: BlobKind::Step
            })
        );
        for name in [
            format!("blobs/{}.step", digest.to_uppercase()),
            format!("blobs/{digest}.stl"),
            format!("blobs/{}.mesh", &digest[1..]),
            format!("blob/{digest}.mesh"),
            format!("blobs/{digest}"),
        ] {
            assert_eq!(BlobName::parse(&name), None, "{name}");
        }
    }

    #[test]
    fn member_roles_follow_the_part_names() {
        let digest = "f".repeat(64);
        for (name, role) in [
            ("container.json", MemberRole::Marker),
            ("document.json", MemberRole::Document),
            ("cache/model.json", MemberRole::ModelManifest),
            ("cache/bodies.bin", MemberRole::ModelBodies),
            ("cache/preview.bin", MemberRole::Preview),
            ("cache/thumbnail.png", MemberRole::Thumbnail),
            ("cache/key.json", MemberRole::Cache),
            ("plugins/io.example/data", MemberRole::Plugin),
            ("provenance/log.cbor", MemberRole::Provenance),
            ("readme.txt", MemberRole::Other),
        ] {
            assert_eq!(MemberRole::of(name), role, "{name}");
        }
        assert!(matches!(
            MemberRole::of(&format!("blobs/{digest}.png")),
            MemberRole::Blob(BlobName {
                kind: BlobKind::Png,
                ..
            })
        ));
    }

    #[test]
    fn markers_are_exact_objects() {
        let of = |value: serde_json::Value| Marker::of(&value.to_string(), Some(&value));
        assert_eq!(
            of(json!({"format": "solidcraft-container/1"})),
            Marker::Design
        );
        assert_eq!(
            of(json!({"format": "solvecraft-container/1"})),
            Marker::DraftDesign
        );
        assert_eq!(
            of(json!({"format": "solidcraft-container/1", "content": "drawing"})),
            Marker::Drawing
        );
        assert!(matches!(
            of(json!({"format": "solidcraft-container/1", "more": 1})),
            Marker::Other(_)
        ));
        assert!(matches!(
            of(json!({"format": "solidcraft-container/2"})),
            Marker::Other(_)
        ));
        assert!(matches!(Marker::of("{", None), Marker::Other(_)));
    }
}

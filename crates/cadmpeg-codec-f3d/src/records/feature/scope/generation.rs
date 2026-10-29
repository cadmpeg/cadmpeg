// SPDX-License-Identifier: Apache-2.0
//! The grammar generation of a parameter-scope frame.
//!
//! The frame length of a scope is a source byte count. Its role in the model
//! is to select the grammar of the scope frame: the fixed frame forms that
//! `docs/formats/f3d.md` names by their length and class pair, or the general
//! envelope, whose length is the sum of its fields. The decoder derives the
//! generation from the frame. The document holds the generation, not the
//! frame length or any frame offset.

use serde::{Deserialize, Serialize};

use super::DesignFeatureKind;

/// The grammar generation of one parameter-scope frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "form", content = "frame", rename_all = "snake_case")]
pub(crate) enum DesignScopeGeneration {
    /// The general envelope: a family prologue, the counted reference table,
    /// the history-state identity, the feature-family name and a fixed or
    /// named tail. No fixed frame form names it.
    Envelope,
    /// A fixed `Assemble` or `As-built` frame.
    Assembly(DesignAssemblyFrame),
    /// A fixed `Component Insert` frame.
    ComponentInsert(DesignComponentInsertFrame),
    /// A fixed legacy `CopyPaste` frame.
    CopyPaste(DesignCopyPasteFrame),
    /// A fixed `JointOrigin` frame.
    JointOrigin(DesignJointOriginFrame),
    /// A class-keyed fixed `Extrude` frame.
    Extrude(DesignExtrudeFrame),
    /// The 290-byte class-301/258 `ReplaceFace` frame.
    ReplaceFace(DesignReplaceFaceFrame),
    /// A `SplitFace` frame, by its length without the references after the
    /// first.
    SplitFace(DesignSplitFaceFrame),
    /// A `DeleteFace` frame, by its base length and base kind offset.
    DeleteFace(DesignDeleteFaceFrame),
    /// A `SurfaceDeleteFace` frame, by its base length and base kind offset.
    SurfaceDeleteFace(DesignSurfaceDeleteFaceFrame),
    /// The single-group path `SurfacePatch` frame.
    SurfacePatch(DesignSurfacePatchFrame),
}

/// The fixed frames of the assembly families (f3d.md "Assembly operands").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DesignAssemblyFrame {
    /// The rigid `As-built` frame of an `members`-member rigid group: 269
    /// bytes for two members and eleven more for each further member.
    Rigid {
        members: u32,
    },
    Frame378,
    Frame399,
    Frame421,
    Frame604,
    Frame627,
    Frame633,
    Frame637,
    Frame671,
    Frame692,
    Frame705,
    Frame732,
    Frame744,
    Frame748,
    Frame772,
    Frame806,
    Frame850,
    Frame861,
    Frame945,
    Frame968,
    Frame1011,
}

/// Byte length of the two-member rigid `As-built` frame.
const RIGID_TWO_MEMBER_LENGTH: u64 = 269;
/// Byte length of one further rigid-group locator reference.
const RIGID_MEMBER_STRIDE: u64 = 11;

impl DesignAssemblyFrame {
    const FIXED: [(Self, u64); 20] = [
        (Self::Frame378, 378),
        (Self::Frame399, 399),
        (Self::Frame421, 421),
        (Self::Frame604, 604),
        (Self::Frame627, 627),
        (Self::Frame633, 633),
        (Self::Frame637, 637),
        (Self::Frame671, 671),
        (Self::Frame692, 692),
        (Self::Frame705, 705),
        (Self::Frame732, 732),
        (Self::Frame744, 744),
        (Self::Frame748, 748),
        (Self::Frame772, 772),
        (Self::Frame806, 806),
        (Self::Frame850, 850),
        (Self::Frame861, 861),
        (Self::Frame945, 945),
        (Self::Frame968, 968),
        (Self::Frame1011, 1011),
    ];

    /// The assembly frame of this length, when f3d.md names one.
    pub(crate) fn from_frame_length(frame_length: u64) -> Option<Self> {
        if let Some((frame, _)) = Self::FIXED
            .iter()
            .find(|(_, length)| *length == frame_length)
        {
            return Some(*frame);
        }
        let extra = frame_length.checked_sub(RIGID_TWO_MEMBER_LENGTH)?;
        if extra % RIGID_MEMBER_STRIDE != 0 {
            return None;
        }
        let members = u32::try_from(extra / RIGID_MEMBER_STRIDE)
            .ok()?
            .checked_add(2)?;
        Some(Self::Rigid { members })
    }

    /// Byte length of the frame.
    pub(crate) fn frame_length(self) -> Option<u64> {
        match self {
            Self::Rigid { members } => u64::from(members.checked_sub(2)?)
                .checked_mul(RIGID_MEMBER_STRIDE)?
                .checked_add(RIGID_TWO_MEMBER_LENGTH),
            fixed => Self::FIXED
                .iter()
                .find(|(frame, _)| *frame == fixed)
                .map(|(_, length)| *length),
        }
    }

    /// The member count of a rigid-group frame.
    pub(crate) fn rigid_members(self) -> Option<usize> {
        match self {
            Self::Rigid { members } => usize::try_from(members).ok(),
            _ => None,
        }
    }
}

/// The fixed `Component Insert` frames (f3d.md "`Component Insert`").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DesignComponentInsertFrame {
    Frame257,
    Frame261,
    Frame267,
    Frame381,
    Frame385,
    Frame389,
    Frame395,
    Frame399,
    Frame404,
}

impl DesignComponentInsertFrame {
    const FIXED: [(Self, u64); 9] = [
        (Self::Frame257, 257),
        (Self::Frame261, 261),
        (Self::Frame267, 267),
        (Self::Frame381, 381),
        (Self::Frame385, 385),
        (Self::Frame389, 389),
        (Self::Frame395, 395),
        (Self::Frame399, 399),
        (Self::Frame404, 404),
    ];
}

/// The fixed legacy `CopyPaste` frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DesignCopyPasteFrame {
    Frame525,
    Frame529,
}

impl DesignCopyPasteFrame {
    const FIXED: [(Self, u64); 2] = [(Self::Frame525, 525), (Self::Frame529, 529)];
}

/// The fixed `JointOrigin` frames (f3d.md "`JointOrigin`" and "Assembly
/// operands").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DesignJointOriginFrame {
    Frame300,
    Frame322,
    Frame336,
    Frame344,
    Frame347,
    Frame366,
    Frame385,
    Frame389,
}

impl DesignJointOriginFrame {
    const FIXED: [(Self, u64); 8] = [
        (Self::Frame300, 300),
        (Self::Frame322, 322),
        (Self::Frame336, 336),
        (Self::Frame344, 344),
        (Self::Frame347, 347),
        (Self::Frame366, 366),
        (Self::Frame385, 385),
        (Self::Frame389, 389),
    ];
}

/// The class-keyed fixed `Extrude` frames (f3d.md "`Extrude`").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DesignExtrudeFrame {
    /// The 538-byte shifted reference-aware two-sided face-target frame of
    /// the classes 357/258, 275/262, 361/262 and 349/266.
    ShiftedToFaces538,
    /// The 516-byte shifted reference-aware two-sided face-target frame of
    /// the class pair 323/263.
    ShiftedToFaces516,
    /// The 485-byte shifted reference-aware symmetric through-all frame of
    /// the class pair 323/263.
    ShiftedThroughAll485,
    /// The 476-byte class-330/258 reference-aware face-target frame.
    ReferenceAware476,
    /// The class-415/265 symmetric-distance frames.
    Class415SymmetricDistance,
    /// The class-415/265 one-sided frames.
    Class415OneSided,
    /// The class-296/261 one-sided to-face frames.
    Class296OneSidedToFace,
    /// The class-296/261 symmetric-distance frame.
    Class296SymmetricDistance,
    /// The class-296/261 two-sided to-faces frame.
    Class296TwoSidedToFaces,
    /// The class-296/261 legacy one-sided to-face frame.
    Class296LegacyOneSidedToFace,
    /// The class-296/261 legacy one-sided distance frame.
    Class296LegacyOneSidedDistance,
    /// The class-397/262 symmetric-distance frame.
    Class397SymmetricDistance,
}

/// The 290-byte class-301/258 `ReplaceFace` frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DesignReplaceFaceFrame {
    Frame290,
}

/// The `SplitFace` frames, by their length less eleven bytes for each
/// reference after the first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DesignSplitFaceFrame {
    Base286,
    Base290,
    Base291,
}

impl DesignSplitFaceFrame {
    const FIXED: [(Self, u64); 3] = [
        (Self::Base286, 286),
        (Self::Base290, 290),
        (Self::Base291, 291),
    ];
}

/// The `DeleteFace` frames, by `(base length, base kind offset)`: the frame
/// length and the kind offset less eleven bytes for each reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DesignDeleteFaceFrame {
    Base232Kind135,
    Base236Kind139,
    Base237Kind139,
    Base241Kind143,
}

impl DesignDeleteFaceFrame {
    const FIXED: [(Self, (u64, u64)); 4] = [
        (Self::Base232Kind135, (232, 135)),
        (Self::Base236Kind139, (236, 139)),
        (Self::Base237Kind139, (237, 139)),
        (Self::Base241Kind143, (241, 143)),
    ];

    /// `(base length, base kind offset)`.
    pub(crate) fn base_layout(self) -> (u64, u64) {
        Self::FIXED
            .iter()
            .find(|(frame, _)| *frame == self)
            .map_or((0, 0), |(_, layout)| *layout)
    }
}

/// The `SurfaceDeleteFace` frames, by `(base length, base kind offset)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DesignSurfaceDeleteFaceFrame {
    Base245Kind135,
    Base246Kind135,
    Base250Kind139,
    Base250Kind140,
    Base251Kind139,
    Base256Kind146,
    Base257Kind146,
}

impl DesignSurfaceDeleteFaceFrame {
    const FIXED: [(Self, (u64, u64)); 7] = [
        (Self::Base245Kind135, (245, 135)),
        (Self::Base246Kind135, (246, 135)),
        (Self::Base250Kind139, (250, 139)),
        (Self::Base250Kind140, (250, 140)),
        (Self::Base251Kind139, (251, 139)),
        (Self::Base256Kind146, (256, 146)),
        (Self::Base257Kind146, (257, 146)),
    ];

    /// `(base length, base kind offset)`.
    pub(crate) fn base_layout(self) -> (u64, u64) {
        Self::FIXED
            .iter()
            .find(|(frame, _)| *frame == self)
            .map_or((0, 0), |(_, layout)| *layout)
    }
}

/// The single-group path `SurfacePatch` frame: 277 bytes and eleven bytes
/// for each reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DesignSurfacePatchFrame {
    GroupedPath,
}

/// Byte length of the single-group path `SurfacePatch` frame without its
/// reference table entries.
const SURFACE_PATCH_GROUPED_PATH_BASE: u64 = 277;
/// Byte length of one plain reference-table entry.
const PLAIN_REFERENCE_LEN: u64 = 11;

/// The frame facts that select a scope generation.
pub(crate) struct ScopeFrameFacts<'a> {
    pub(crate) kind: &'a DesignFeatureKind,
    pub(crate) class_tag: &'a str,
    pub(crate) paired_class_tag: &'a str,
    pub(crate) frame_length: u64,
    /// Offset of the reference count from the primary header.
    pub(crate) reference_count_delta: u64,
    /// Offset of the kind code units from the primary header.
    pub(crate) kind_delta: u64,
    pub(crate) reference_count: usize,
}

fn fixed<T: Copy>(table: &[(T, u64)], frame_length: u64) -> Option<T> {
    table
        .iter()
        .find(|(_, length)| *length == frame_length)
        .map(|(frame, _)| *frame)
}

impl DesignScopeGeneration {
    /// Derive the generation from the decoded frame.
    pub(crate) fn derive(facts: &ScopeFrameFacts<'_>) -> Self {
        use DesignFeatureKind as Kind;
        let reference_bytes = u64::try_from(facts.reference_count)
            .ok()
            .and_then(|count| count.checked_mul(PLAIN_REFERENCE_LEN));
        let derived = match facts.kind {
            Kind::Assemble | Kind::AsBuilt => {
                DesignAssemblyFrame::from_frame_length(facts.frame_length).map(Self::Assembly)
            }
            Kind::ComponentInsert => fixed(&DesignComponentInsertFrame::FIXED, facts.frame_length)
                .map(Self::ComponentInsert),
            Kind::CopyPaste => {
                fixed(&DesignCopyPasteFrame::FIXED, facts.frame_length).map(Self::CopyPaste)
            }
            Kind::JointOrigin => {
                fixed(&DesignJointOriginFrame::FIXED, facts.frame_length).map(Self::JointOrigin)
            }
            Kind::Extrude | Kind::Extrusion | Kind::Extrusao => {
                DesignExtrudeFrame::derive(facts).map(Self::Extrude)
            }
            Kind::ReplaceFace => (facts.class_tag == "301"
                && facts.paired_class_tag == "258"
                && facts.frame_length == 290)
                .then_some(Self::ReplaceFace(DesignReplaceFaceFrame::Frame290)),
            Kind::SplitFace => facts
                .reference_count
                .checked_sub(1)
                .and_then(|count| u64::try_from(count).ok())
                .and_then(|count| count.checked_mul(PLAIN_REFERENCE_LEN))
                .and_then(|tail| facts.frame_length.checked_sub(tail))
                .and_then(|base| fixed(&DesignSplitFaceFrame::FIXED, base))
                .map(Self::SplitFace),
            Kind::DeleteFace => reference_bytes
                .and_then(|bytes| {
                    Some((
                        facts.frame_length.checked_sub(bytes)?,
                        facts.kind_delta.checked_sub(bytes)?,
                    ))
                })
                .and_then(|layout| {
                    DesignDeleteFaceFrame::FIXED
                        .iter()
                        .find(|(_, base)| *base == layout)
                        .map(|(frame, _)| *frame)
                })
                .map(Self::DeleteFace),
            Kind::SurfaceDeleteFace => reference_bytes
                .and_then(|bytes| {
                    Some((
                        facts.frame_length.checked_sub(bytes)?,
                        facts.kind_delta.checked_sub(bytes)?,
                    ))
                })
                .and_then(|layout| {
                    DesignSurfaceDeleteFaceFrame::FIXED
                        .iter()
                        .find(|(_, base)| *base == layout)
                        .map(|(frame, _)| *frame)
                })
                .map(Self::SurfaceDeleteFace),
            Kind::SurfacePatch => reference_bytes
                .and_then(|bytes| bytes.checked_add(SURFACE_PATCH_GROUPED_PATH_BASE))
                .filter(|length| *length == facts.frame_length)
                .map(|_| Self::SurfacePatch(DesignSurfacePatchFrame::GroupedPath)),
            _ => None,
        };
        derived.unwrap_or(Self::Envelope)
    }

    /// Whether this generation belongs to the feature family `kind`.
    pub(crate) fn admits_kind(self, kind: &DesignFeatureKind) -> bool {
        use DesignFeatureKind as Kind;
        match self {
            Self::Envelope => true,
            Self::Assembly(_) => matches!(kind, Kind::Assemble | Kind::AsBuilt),
            Self::ComponentInsert(_) => *kind == Kind::ComponentInsert,
            Self::CopyPaste(_) => *kind == Kind::CopyPaste,
            Self::JointOrigin(_) => *kind == Kind::JointOrigin,
            Self::Extrude(_) => matches!(kind, Kind::Extrude | Kind::Extrusion | Kind::Extrusao),
            Self::ReplaceFace(_) => *kind == Kind::ReplaceFace,
            Self::SplitFace(_) => *kind == Kind::SplitFace,
            Self::DeleteFace(_) => *kind == Kind::DeleteFace,
            Self::SurfaceDeleteFace(_) => *kind == Kind::SurfaceDeleteFace,
            Self::SurfacePatch(_) => *kind == Kind::SurfacePatch,
        }
    }

    /// Whether a scope with the class pair `class_tag`/`paired_class_tag`
    /// can have this generation.
    pub(crate) fn admits_classes(self, class_tag: &str, paired_class_tag: &str) -> bool {
        match self {
            Self::Extrude(frame) => frame.admits_classes(class_tag, paired_class_tag),
            Self::ReplaceFace(DesignReplaceFaceFrame::Frame290) => {
                (class_tag, paired_class_tag) == ("301", "258")
            }
            _ => true,
        }
    }

    /// The assembly frame, for an assembly generation.
    pub(crate) fn assembly(self) -> Option<DesignAssemblyFrame> {
        match self {
            Self::Assembly(frame) => Some(frame),
            _ => None,
        }
    }

    /// The `Extrude` frame, for a class-keyed fixed `Extrude` generation.
    pub(crate) fn extrude(self) -> Option<DesignExtrudeFrame> {
        match self {
            Self::Extrude(frame) => Some(frame),
            _ => None,
        }
    }
}

impl DesignExtrudeFrame {
    /// Whether a scope with this class pair can have this frame.
    fn admits_classes(self, class_tag: &str, paired_class_tag: &str) -> bool {
        let classes = (class_tag, paired_class_tag);
        match self {
            Self::ShiftedToFaces538 => matches!(
                classes,
                ("357", "258") | ("275" | "361", "262") | ("349", "266")
            ),
            Self::ShiftedToFaces516 | Self::ShiftedThroughAll485 => classes == ("323", "263"),
            Self::ReferenceAware476 => classes == ("330", "258"),
            Self::Class415SymmetricDistance | Self::Class415OneSided => classes == ("415", "265"),
            Self::Class296OneSidedToFace
            | Self::Class296SymmetricDistance
            | Self::Class296TwoSidedToFaces
            | Self::Class296LegacyOneSidedToFace
            | Self::Class296LegacyOneSidedDistance => classes == ("296", "261"),
            Self::Class397SymmetricDistance => classes == ("397", "262"),
        }
    }

    fn derive(facts: &ScopeFrameFacts<'_>) -> Option<Self> {
        use crate::design::decode::scopes::extrude as layouts;
        use crate::design::decode::scopes::legacy_class_397::Class397SymmetricFrame;
        use crate::design::decode::scopes::legacy_class_415;
        let classes = (facts.class_tag, facts.paired_class_tag);
        let layout = (
            facts.class_tag,
            facts.paired_class_tag,
            facts.frame_length,
            facts.reference_count_delta,
            facts.reference_count,
        );
        let (class_tag, paired_class_tag, frame_length, delta, count) = layout;
        let derived = match (classes, frame_length) {
            (("357", "258") | ("275" | "361", "262") | ("349", "266"), 538) => {
                Self::ShiftedToFaces538
            }
            (("323", "263"), 516) => Self::ShiftedToFaces516,
            (("323", "263"), 485) => Self::ShiftedThroughAll485,
            (("330", "258"), 476) => Self::ReferenceAware476,
            _ if legacy_class_415::is_symmetric_distance_layout(
                class_tag,
                paired_class_tag,
                frame_length,
                delta,
                count,
            ) =>
            {
                Self::Class415SymmetricDistance
            }
            _ if legacy_class_415::is_one_sided_layout(
                class_tag,
                paired_class_tag,
                frame_length,
                delta,
                count,
            ) =>
            {
                Self::Class415OneSided
            }
            _ if layouts::is_class_296_one_sided_to_face_layout(
                class_tag,
                paired_class_tag,
                frame_length,
                delta,
                count,
            ) =>
            {
                Self::Class296OneSidedToFace
            }
            _ if layouts::is_class_296_symmetric_distance_layout(
                class_tag,
                paired_class_tag,
                frame_length,
                delta,
                count,
            ) =>
            {
                Self::Class296SymmetricDistance
            }
            _ if layouts::is_class_296_two_sided_to_faces_layout(
                class_tag,
                paired_class_tag,
                frame_length,
                delta,
                count,
            ) =>
            {
                Self::Class296TwoSidedToFaces
            }
            _ if layouts::is_class_296_legacy_one_sided_to_face_layout(
                class_tag,
                paired_class_tag,
                frame_length,
                delta,
                count,
            ) =>
            {
                Self::Class296LegacyOneSidedToFace
            }
            _ if layouts::is_class_296_legacy_one_sided_distance_layout(
                class_tag,
                paired_class_tag,
                frame_length,
                delta,
                count,
            ) =>
            {
                Self::Class296LegacyOneSidedDistance
            }
            _ if Class397SymmetricFrame::new(
                class_tag,
                paired_class_tag,
                frame_length,
                delta,
                count,
            )
            .is_some() =>
            {
                Self::Class397SymmetricDistance
            }
            _ => return None,
        };
        Some(derived)
    }
}

impl DesignComponentInsertFrame {
    /// Byte length of the frame.
    pub(crate) fn frame_length(self) -> u64 {
        Self::FIXED
            .iter()
            .find(|(frame, _)| *frame == self)
            .map_or(0, |(_, length)| *length)
    }
}

impl DesignJointOriginFrame {
    /// Byte length of the frame.
    pub(crate) fn frame_length(self) -> u64 {
        Self::FIXED
            .iter()
            .find(|(frame, _)| *frame == self)
            .map_or(0, |(_, length)| *length)
    }
}

#[cfg(test)]
mod tests;

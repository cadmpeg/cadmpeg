// SPDX-License-Identifier: Apache-2.0
//! Sketch visibility, placement and the frame a sketch is placed on.

use super::identity::{DesignAffineTransform, DesignEntityId, IDENTITY_MATRIX};
use super::references::DesignClassTag;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;

cadmpeg_core::named_optional_field!(deserialize_scope_record_index, u32, "scope_record_index");
cadmpeg_core::named_optional_field!(deserialize_visibility, DesignSketchVisibility, "visibility");
cadmpeg_core::named_optional_field!(
    deserialize_ordinal_collection,
    std::num::NonZeroU64,
    "ordinal_collection"
);
/// Typed sketch-container visibility bound to a Design sketch entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignSketchVisibilityWire",
    into = "DesignSketchVisibilityWire"
)]
pub(crate) struct DesignSketchVisibility {
    /// One-based ordinal among sketch Geometry members in the referenced collection.
    pub(crate) stream_ordinal: NonZeroU32,
    stream_ordinal_offset: Option<u64>,
    /// Direct display visibility.
    pub(crate) visible: bool,
    pub(crate) ordinal_collection: Option<std::num::NonZeroU64>,
}

impl DesignSketchVisibility {
    pub(crate) fn new(
        stream_ordinal: NonZeroU32,
        stream_ordinal_offset: u64,
        visible: bool,
    ) -> Result<Self, String> {
        stream_ordinal_offset
            .checked_add(5)
            .ok_or("stream_ordinal_offset overflows visible_offset")?;
        Ok(Self {
            stream_ordinal,
            stream_ordinal_offset: Some(stream_ordinal_offset),
            visible,
            ordinal_collection: None,
        })
    }

    pub(crate) fn stream_ordinal_offset(&self) -> Option<u64> {
        self.stream_ordinal_offset
    }

    pub(crate) fn visible_offset(&self) -> Option<u64> {
        self.stream_ordinal_offset?.checked_add(5)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DesignSketchVisibilityWire {
    stream_ordinal: u32,
    visible: bool,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_ordinal_collection"
    )]
    ordinal_collection: Option<std::num::NonZeroU64>,
}
impl TryFrom<DesignSketchVisibilityWire> for DesignSketchVisibility {
    type Error = String;
    fn try_from(wire: DesignSketchVisibilityWire) -> Result<Self, Self::Error> {
        Ok(Self {
            stream_ordinal: NonZeroU32::new(wire.stream_ordinal)
                .ok_or("stream_ordinal must be nonzero")?,
            stream_ordinal_offset: None,
            visible: wire.visible,
            ordinal_collection: wire.ordinal_collection,
        })
    }
}
impl From<DesignSketchVisibility> for DesignSketchVisibilityWire {
    fn from(value: DesignSketchVisibility) -> Self {
        Self {
            stream_ordinal: value.stream_ordinal.get(),
            visible: value.visible,
            ordinal_collection: value.ordinal_collection,
        }
    }
}

/// Local-to-model placement frame referenced by a Design sketch scope.
#[derive(Debug, PartialEq, Deserialize)]
#[cfg_attr(not(test), derive(Clone))]
#[serde(try_from = "DesignSketchPlacementWire")]
pub(crate) struct DesignSketchPlacement {
    /// Globally unique deterministic identifier for this native record.
    pub(crate) id: String,
    /// Owning parameter scope, when the sketch has a parameter scope.
    pub(crate) scope_record_index: Option<u32>,
    /// Full Design entity id of the placed sketch.
    pub(crate) entity_id: DesignEntityId,
    /// Typed sketch-container visibility for the placed sketch entity.
    pub(crate) visibility: Option<DesignSketchVisibility>,
    /// Source dynamic three-digit ASCII primary class tag.
    pub(crate) class_tag: DesignClassTag,
    /// Shared logical record identity.
    pub(crate) record_index: u32,
    /// Source dynamic class tag of the paired header.
    pub(crate) paired_class_tag: DesignClassTag,
    /// Source layout and its checked placement matrix and byte extent.
    pub(crate) frame: DesignSketchFrame,
}

#[cfg(test)]
thread_local! {
    static SKETCH_PLACEMENT_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl Clone for DesignSketchPlacement {
    fn clone(&self) -> Self {
        SKETCH_PLACEMENT_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            scope_record_index: self.scope_record_index,
            entity_id: self.entity_id.clone(),
            visibility: self.visibility,
            class_tag: self.class_tag.clone(),
            record_index: self.record_index,
            paired_class_tag: self.paired_class_tag.clone(),
            frame: self.frame.clone(),
        }
    }
}

pub(crate) fn valid_sketch_transform(transform: &[[f64; 4]; 4]) -> bool {
    const EPSILON: f64 = 1.0e-10;
    if !transform.iter().flatten().all(|value| value.is_finite())
        || transform[3] != [0.0, 0.0, 0.0, 1.0]
    {
        return false;
    }
    let columns = [
        [transform[0][0], transform[1][0], transform[2][0]],
        [transform[0][1], transform[1][1], transform[2][1]],
        [transform[0][2], transform[1][2], transform[2][2]],
    ];
    for (ordinal, column) in columns.iter().enumerate() {
        let norm = column.iter().map(|value| value * value).sum::<f64>();
        if (norm - 1.0).abs() > EPSILON {
            return false;
        }
        for other in &columns[..ordinal] {
            let dot = column
                .iter()
                .zip(other)
                .map(|(left, right)| left * right)
                .sum::<f64>();
            if dot.abs() > EPSILON {
                return false;
            }
        }
    }
    true
}

/// A finite affine placement with orthonormal basis columns.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "[[f64; 4]; 4]", into = "[[f64; 4]; 4]")]
pub(crate) struct SketchPlacementMatrix(DesignAffineTransform);

impl SketchPlacementMatrix {
    /// The identity placement.
    pub(crate) const IDENTITY: Self = Self(DesignAffineTransform::IDENTITY);
    /// The row-major matrix coefficients.
    pub(crate) fn rows(self) -> [[f64; 4]; 4] {
        self.0.rows()
    }
    /// The matrix rows in storage order.
    #[cfg(test)]
    pub(crate) fn iter(&self) -> std::slice::Iter<'_, [f64; 4]> {
        self.0.iter()
    }
}

impl AsRef<[[f64; 4]; 4]> for SketchPlacementMatrix {
    fn as_ref(&self) -> &[[f64; 4]; 4] {
        &self.0
    }
}

impl std::ops::Index<usize> for SketchPlacementMatrix {
    type Output = [f64; 4];
    fn index(&self, index: usize) -> &Self::Output {
        &self.0[index]
    }
}

impl From<SketchPlacementMatrix> for [[f64; 4]; 4] {
    fn from(matrix: SketchPlacementMatrix) -> Self {
        matrix.0.rows()
    }
}

impl TryFrom<[[f64; 4]; 4]> for SketchPlacementMatrix {
    type Error = String;
    fn try_from(value: [[f64; 4]; 4]) -> Result<Self, Self::Error> {
        if valid_sketch_transform(&value) {
            Ok(Self(DesignAffineTransform::try_from(value)?))
        } else {
            Err("transform must be a finite affine matrix with orthonormal columns".into())
        }
    }
}

/// The matrix-bearing payload of each source placement layout.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum DesignSketchFrameForm {
    /// Document-authored content has no implied source encoding or location.
    Authored {
        transform: SketchPlacementMatrix,
        member_run_head: bool,
    },
    ScopeCompact,
    ScopeGenesisCompact,
    ScopeLegacy305(SketchPlacementMatrix),
    ScopeLegacy325(SketchPlacementMatrix),
    ScopeExplicit(SketchPlacementMatrix),
    ScopeGenesisExplicit(SketchPlacementMatrix),
    MemberCompact {
        paired_byte_offset: u64,
    },
    MemberExplicit {
        paired_byte_offset: u64,
        transform: SketchPlacementMatrix,
        /// The closing head reference carries an inline type GUID, making
        /// the frame forty bytes longer.
        typed_reference: bool,
    },
}

/// A placement layout whose complete byte extent fits in its address space.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DesignSketchFrame {
    byte_offset: Option<u64>,
    form: DesignSketchFrameForm,
}

impl DesignSketchFrame {
    pub(crate) fn form(&self) -> &DesignSketchFrameForm {
        &self.form
    }
    pub(crate) fn new(byte_offset: u64, form: DesignSketchFrameForm) -> Result<Self, String> {
        let value = Self {
            byte_offset: Some(byte_offset),
            form,
        };
        byte_offset
            .checked_add(
                value
                    .frame_length()
                    .ok_or("authored placement has no source frame extent")?,
            )
            .ok_or("byte_offset overflows the sketch frame extent")?;
        if !value
            .canonical_transform()
            .iter()
            .flatten()
            .all(|value| value.is_finite())
        {
            return Err("transform translation overflows canonical millimeters".into());
        }
        Ok(value)
    }
    pub(crate) fn canonical_transform(&self) -> [[f64; 4]; 4] {
        let mut rows = *self.transform();
        let scale = match self.form {
            DesignSketchFrameForm::ScopeGenesisCompact
            | DesignSketchFrameForm::ScopeGenesisExplicit(_)
            | DesignSketchFrameForm::MemberCompact { .. }
            | DesignSketchFrameForm::MemberExplicit { .. } => 10.0,
            _ => 1.0,
        };
        for row in rows.iter_mut().take(3) {
            row[3] *= scale;
        }
        rows
    }
    fn frame_length(&self) -> Option<u64> {
        Some(match self.form {
            DesignSketchFrameForm::Authored { .. } => return None,
            DesignSketchFrameForm::ScopeCompact => 201,
            DesignSketchFrameForm::ScopeGenesisCompact => 213,
            DesignSketchFrameForm::ScopeLegacy305(_) => 305,
            DesignSketchFrameForm::ScopeLegacy325(_) => 325,
            DesignSketchFrameForm::ScopeExplicit(_) => 329,
            DesignSketchFrameForm::ScopeGenesisExplicit(_) => 341,
            DesignSketchFrameForm::MemberCompact { .. } => 34,
            DesignSketchFrameForm::MemberExplicit {
                typed_reference, ..
            } => {
                if typed_reference {
                    202
                } else {
                    162
                }
            }
        })
    }
    fn transform(&self) -> &[[f64; 4]; 4] {
        match &self.form {
            DesignSketchFrameForm::ScopeCompact
            | DesignSketchFrameForm::ScopeGenesisCompact
            | DesignSketchFrameForm::MemberCompact { .. } => &IDENTITY_MATRIX,
            DesignSketchFrameForm::Authored {
                transform: matrix, ..
            }
            | DesignSketchFrameForm::ScopeLegacy305(matrix)
            | DesignSketchFrameForm::ScopeLegacy325(matrix)
            | DesignSketchFrameForm::ScopeExplicit(matrix)
            | DesignSketchFrameForm::ScopeGenesisExplicit(matrix)
            | DesignSketchFrameForm::MemberExplicit {
                transform: matrix, ..
            } => &matrix.0,
        }
    }
    #[cfg(test)]
    fn transform_offset(&self) -> Option<u64> {
        let relative = match self.form {
            DesignSketchFrameForm::Authored { .. } => return None,
            DesignSketchFrameForm::ScopeCompact
            | DesignSketchFrameForm::ScopeGenesisCompact
            | DesignSketchFrameForm::MemberCompact { .. } => return None,
            DesignSketchFrameForm::ScopeLegacy305(_) | DesignSketchFrameForm::ScopeLegacy325(_) => {
                48
            }
            DesignSketchFrameForm::ScopeExplicit(_) => 55,
            DesignSketchFrameForm::ScopeGenesisExplicit(_) => 66,
            DesignSketchFrameForm::MemberExplicit { .. } => 22,
        };
        self.byte_offset?.checked_add(relative)
    }
    #[cfg(test)]
    pub(super) fn paired_byte_offset(&self) -> Option<u64> {
        match self.form {
            DesignSketchFrameForm::MemberCompact { paired_byte_offset }
            | DesignSketchFrameForm::MemberExplicit {
                paired_byte_offset, ..
            } => Some(paired_byte_offset),
            _ => self.byte_offset?.checked_add(self.frame_length()?),
        }
    }
}

impl DesignSketchPlacement {
    pub(crate) fn byte_offset(&self) -> Option<u64> {
        self.frame.byte_offset
    }
    #[cfg(test)]
    pub(crate) fn frame_length(&self) -> Option<u64> {
        self.frame.frame_length()
    }
    pub(crate) fn transform(&self) -> &[[f64; 4]; 4] {
        self.frame.transform()
    }
    #[cfg(test)]
    pub(crate) fn transform_offset(&self) -> Option<u64> {
        self.frame.transform_offset()
    }
    #[cfg(test)]
    pub(crate) fn paired_byte_offset(&self) -> Option<u64> {
        self.frame.paired_byte_offset()
    }
    pub(crate) fn member_run_head(&self) -> bool {
        match self.frame.form {
            DesignSketchFrameForm::Authored {
                member_run_head, ..
            } => member_run_head,
            DesignSketchFrameForm::MemberCompact { .. }
            | DesignSketchFrameForm::MemberExplicit { .. } => true,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DesignSketchPlacementWire {
    id: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_scope_record_index"
    )]
    scope_record_index: Option<u32>,
    entity_id: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_visibility"
    )]
    visibility: Option<DesignSketchVisibility>,
    class_tag: String,
    record_index: u32,
    transform: [[f64; 4]; 4],
    paired_class_tag: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    member_run_head: bool,
}
impl Serialize for DesignSketchPlacement {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct WireRef<'a> {
            id: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            scope_record_index: Option<u32>,
            entity_id: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            visibility: Option<DesignSketchVisibility>,
            class_tag: &'a str,
            record_index: u32,
            transform: [[f64; 4]; 4],
            paired_class_tag: &'a str,
            #[serde(skip_serializing_if = "std::ops::Not::not")]
            member_run_head: bool,
        }
        WireRef {
            id: &self.id,
            scope_record_index: self.scope_record_index,
            entity_id: self.entity_id.as_str(),
            visibility: self.visibility,
            class_tag: self.class_tag.as_str(),
            record_index: self.record_index,
            transform: self.frame.canonical_transform(),
            paired_class_tag: self.paired_class_tag.as_str(),
            member_run_head: self.member_run_head(),
        }
        .serialize(serializer)
    }
}
impl TryFrom<DesignSketchPlacementWire> for DesignSketchPlacement {
    type Error = String;
    fn try_from(wire: DesignSketchPlacementWire) -> Result<Self, Self::Error> {
        Ok(Self {
            id: wire.id,
            scope_record_index: wire.scope_record_index,
            entity_id: DesignEntityId::try_from(wire.entity_id)?,
            visibility: wire.visibility,
            class_tag: wire.class_tag.try_into()?,
            record_index: wire.record_index,
            paired_class_tag: wire
                .paired_class_tag
                .try_into()
                .map_err(|error| format!("paired_class_tag: {error}"))?,
            frame: DesignSketchFrame {
                byte_offset: None,
                form: DesignSketchFrameForm::Authored {
                    transform: wire.transform.try_into()?,
                    member_run_head: wire.member_run_head,
                },
            },
        })
    }
}
#[cfg(test)]
impl From<DesignSketchPlacement> for DesignSketchPlacementWire {
    fn from(value: DesignSketchPlacement) -> Self {
        let transform = value.frame.canonical_transform();
        let member_run_head = value.member_run_head();
        Self {
            id: value.id,
            scope_record_index: value.scope_record_index,
            entity_id: value.entity_id.into_string(),
            visibility: value.visibility,
            class_tag: value.class_tag.into(),
            record_index: value.record_index,
            transform,
            paired_class_tag: value.paired_class_tag.into(),
            member_run_head,
        }
    }
}

#[cfg(test)]
mod tests;

mod identity_rewrite;

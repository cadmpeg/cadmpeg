// SPDX-License-Identifier: Apache-2.0
//! Ordered plate, bend, and corner children of a flange or contour flange.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};

use crate::pmdc::{
    reference_list_with_context, state_content_header, Cursor, PmDcContentHeader, PmDcReferenceList,
};
use crate::record_identity::{Located, RecordPayload};

pub(crate) const TYPE: [u8; 16] = [
    0x08, 0xdc, 0xdd, 0xc3, 0xd3, 0x11, 0xd0, 0x97, 0x60, 0x00, 0xa7, 0xa9, 0x9d, 0xcc, 0xef, 0xb0,
];

/// The contour-flange parent. Its payload has the flange layout.
pub(crate) const CONTOUR_TYPE: [u8; 16] = [
    0xb0, 0x74, 0x38, 0xa0, 0xd4, 0x11, 0x38, 0x12, 0x60, 0x00, 0x18, 0xaa, 0x9d, 0xcc, 0xef, 0xb0,
];

/// The sheet-metal face parent. Its payload has the flange layout.
pub(crate) const FACE_TYPE: [u8; 16] = [
    0x90, 0x4b, 0xc1, 0xc4, 0xd3, 0x11, 0xff, 0x28, 0x60, 0x00, 0x4d, 0xa9, 0x9d, 0xcc, 0xef, 0xb0,
];

pub(crate) type Flange = Located<FlangePayload>;

/// The parent operation that owns the plate, bend, and corner children.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FlangeKind {
    /// Type `08dcddc3d311d0976000a7a99dccefb0`: a flange on existing edges.
    Edge,
    /// Type `b07438a0d4113812600018aa9dccefb0`: a contour flange from an
    /// open sketch profile.
    Contour,
    /// Type `904bc1c4d311ff2860004da99dccefb0`: a sheet-metal face. It
    /// names the plate, or the plate and bend, that it holds.
    Face,
}

impl FlangeKind {
    /// The kind of a parsed parent record.
    pub(crate) fn of(flange: &Flange) -> Option<Self> {
        let type_id = flange.identity.type_id.as_str();
        [Self::Edge, Self::Contour, Self::Face]
            .into_iter()
            .find(|kind| type_id == crate::pmdc::type_id_string(kind.type_id()))
    }

    /// The record type of this kind.
    pub(crate) const fn type_id(self) -> [u8; 16] {
        match self {
            Self::Edge => TYPE,
            Self::Contour => CONTOUR_TYPE,
            Self::Face => FACE_TYPE,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FlangePayload {
    save_version_major: u8,
    pub(crate) header: PmDcContentHeader,
    state: i32,
    pub(crate) children: PmDcReferenceList,
    value: u32,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_attachment_edges"
    )]
    pub(crate) attachment_edges: Option<super::contour::AttachmentEdges>,
}
cadmpeg_core::named_optional_field!(
    deserialize_attachment_edges,
    super::contour::AttachmentEdges,
    "attachment_edges"
);

impl RecordPayload for FlangePayload {
    const KIND: &'static str = "sheet-metal-flange";
}

pub(crate) fn parse(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<FlangePayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = state_content_header(&mut cursor, version)?;
    let state = cursor.i32("flange state")?;
    let children = reference_list_with_context(ctx, &mut cursor, 2, "flange children")?;
    let value = cursor.u32("flange value")?;
    cursor.finish("sheet-metal flange")?;
    Ok(FlangePayload {
        save_version_major: version,
        header,
        state,
        children,
        value,
        attachment_edges: None,
    })
}

impl Flange {
    pub(super) fn with_attachment_edges(
        &self,
        edges: Option<super::contour::AttachmentEdges>,
    ) -> Self {
        let mut payload = (**self).clone();
        payload.attachment_edges = edges;
        Self::new(
            payload,
            self.identity.type_id.clone(),
            self.identity.segment_token.clone(),
            self.identity.record_ordinal,
        )
    }
}

pub(crate) fn valid<'a>(flange: &Flange, type_at: impl Fn(u32) -> Option<&'a str>) -> bool {
    let Some(kind) = FlangeKind::of(flange) else {
        return false;
    };
    type_at(flange.identity.record_ordinal)
        .is_some_and(|actual| actual == crate::pmdc::type_id_string(kind.type_id()))
        && [flange.header.next, flange.header.context]
            .into_iter()
            .all(|reference| {
                reference
                    .record_ordinal()
                    .is_none_or(|ordinal| type_at(ordinal).is_some())
            })
        && flange.children.references().iter().all(|reference| {
            reference.record_ordinal().is_none_or(|ordinal| {
                type_at(ordinal).is_some_and(|actual| {
                    actual == crate::pmdc::type_id_string(super::super::FEATURE_TYPE)
                })
            })
        })
}

#[cfg(test)]
mod tests;

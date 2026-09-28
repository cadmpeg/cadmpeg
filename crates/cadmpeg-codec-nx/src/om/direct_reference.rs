// SPDX-License-Identifier: Apache-2.0
//! Exact direct operation reference fields and their derived positions.

use super::reference_index::CanonicalFeatureReferenceToken;
use crate::om::operation_record::OperationPayload;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use std::ops::Add;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReferenceFieldKind {
    Tagged17,
    DataBlock03,
}

impl ReferenceFieldKind {
    pub(crate) fn from_tag(tag: Option<u8>) -> Result<Self, &'static str> {
        match tag {
            None => Ok(Self::DataBlock03),
            Some(0x17) => Ok(Self::Tagged17),
            _ => Err("tag: unsupported direct reference field"),
        }
    }
    pub(crate) fn tag(self) -> Option<u8> {
        match self {
            Self::Tagged17 => Some(0x17),
            Self::DataBlock03 => None,
        }
    }
    fn prefix(self) -> [u8; 3] {
        match self {
            Self::Tagged17 => [1, 2, 0x17],
            Self::DataBlock03 => [1, 2, 3],
        }
    }
    fn suffix(self) -> &'static [u8] {
        match self {
            Self::Tagged17 => &[0xff, 0x80, 0, 0, 2],
            Self::DataBlock03 => &[1, 0, 0, 0, 0, 0],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DirectReferenceFrame<O> {
    kind: ReferenceFieldKind,
    object: CanonicalFeatureReferenceToken,
    offset: O,
}

impl<O> DirectReferenceFrame<O> {
    pub(crate) fn kind(&self) -> ReferenceFieldKind {
        self.kind
    }
    pub(crate) fn object(&self) -> CanonicalFeatureReferenceToken {
        self.object
    }
    pub(crate) fn byte_len(&self) -> u8 {
        3 + self.object.raw().len() as u8 + self.kind.suffix().len() as u8
    }
}

impl<O: Copy + Add<Output = O> + From<u8>> DirectReferenceFrame<O> {
    pub(crate) fn offset(&self) -> O {
        self.offset
    }
    pub(crate) fn object_offset(&self) -> O {
        self.offset + O::from(3)
    }
}

macro_rules! positioned_frame {
    ($offset:ty) => {
        impl DirectReferenceFrame<$offset> {
            pub(crate) fn new(
                kind: ReferenceFieldKind,
                object: CanonicalFeatureReferenceToken,
                offset: $offset,
            ) -> Option<Self> {
                let frame = Self {
                    kind,
                    object,
                    offset,
                };
                offset.checked_add(<$offset>::from(frame.byte_len()))?;
                Some(frame)
            }
        }
    };
}
positioned_frame!(usize);
positioned_frame!(u64);

/// Retain fields with their exact suffix; assign no endpoint or operation role.
pub(crate) fn operation_reference_fields(
    ctx: &DecodeContext<'_>,
    record: OperationPayload<'_>,
    kind: ReferenceFieldKind,
) -> Result<Vec<DirectReferenceFrame<usize>>, CodecError> {
    let prefix = kind.prefix();
    let mut fields = Vec::new();
    for (marker, window) in record.payload()
        .windows(prefix.len())
        .enumerate()
    {
        if window != prefix { continue; }
        let Some(token) = marker.checked_add(prefix.len()) else { continue; };
        let Some(object) = record.payload().get(token..).and_then(CanonicalFeatureReferenceToken::read) else { continue; };
        let Some(end) = token.checked_add(object.raw().len()) else { continue; };
        let Some(suffix_end) = end.checked_add(kind.suffix().len()) else { continue; };
        if record.payload().get(end..suffix_end) != Some(kind.suffix()) { continue; }
        let Some(frame) = record.payload_offset().checked_add(marker).and_then(|offset| DirectReferenceFrame::<usize>::new(
                kind,
                object,
                offset,
            )) else { continue; };
        super::reserve_om_retained_item(ctx, &mut fields, "nx direct reference fields")?;
        fields.push(frame);
    }
    Ok(fields)
}

#[cfg(test)]
mod tests {
    // Preserve the fixture suite's existing unwrap lint policy.
    #![allow(clippy::unwrap_used)]
    mod operation_data_block_references;
    mod tagged_references;
    use super::super::reference_index::CanonicalFeatureReferenceToken;
    use super::{DirectReferenceFrame, ReferenceFieldKind};
    use crate::om::operation_record::OperationPayload;

    fn record(payload: &[u8], payload_offset: usize) -> OperationPayload<'_> {
        OperationPayload::new(payload, payload_offset, "EXTRUDE").unwrap()
    }

    #[test]
    fn direct_reference_positions_bound_each_field_kind() {
        let object = CanonicalFeatureReferenceToken::from_wire(1, &[1]).unwrap();
        for (kind, length) in [
            (ReferenceFieldKind::Tagged17, 9),
            (ReferenceFieldKind::DataBlock03, 10),
        ] {
            let frame = DirectReferenceFrame::<usize>::new(kind, object, 100).unwrap();
            assert_eq!(frame.object_offset(), 103);
            assert_eq!(frame.byte_len(), length);
            assert!(DirectReferenceFrame::<usize>::new(
                kind,
                object,
                usize::MAX - usize::from(length)
            )
            .is_some());
            assert!(DirectReferenceFrame::<usize>::new(
                kind,
                object,
                usize::MAX - usize::from(length) + 1
            )
            .is_none());
            assert!(
                DirectReferenceFrame::<u64>::new(kind, object, u64::MAX - u64::from(length))
                    .is_some()
            );
            assert!(DirectReferenceFrame::<u64>::new(
                kind,
                object,
                u64::MAX - u64::from(length) + 1
            )
            .is_none());
        }
    }

    #[test]
    fn direct_reference_fields_refuse_collection_limit() {
        let bytes = [1, 2, 3, 7, 1, 0, 0, 0, 0, 0];
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        let error = super::operation_reference_fields(&ctx, record(&bytes, 0), ReferenceFieldKind::DataBlock03).unwrap_err();
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
    }

    #[test]
    fn direct_reference_fields_refuse_retained_limit() {
        let bytes = [1, 2, 3, 7, 1, 0, 0, 0, 0, 0];
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        let error = super::operation_reference_fields(&ctx, record(&bytes, 0), ReferenceFieldKind::DataBlock03).unwrap_err();
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
    }
}

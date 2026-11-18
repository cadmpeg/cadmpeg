// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;

use cadmpeg_core::decode::u64_from_index;
use cadmpeg_ir::native::bytes::NativeBytes;
use serde::{Deserialize, Serialize};

use crate::bytes::local_reference_target;
use crate::layout::{
    joint_origin_direct_carrier_369 as short, joint_origin_direct_carrier_391 as long,
    joint_origin_direct_carrier_412 as extended, joint_origin_scope_carrier_separation,
};
use crate::records::feature::scope::{
    DesignFeatureKind, DesignJointOriginTransform, DesignParameterScope,
};

const SCOPE_TYPE_GUID: &str = "02E8AA49-08B8-439C-A8C7-221BE20143D8";
const PLACEMENT_TYPE_GUID: &str = "7B6C3C2D-4096-4E6D-B225-699E8C2D9355";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignJointOriginCarrierEvidence {
    pub(crate) frame_length: u64,
    pub(crate) prefix: NativeBytes,
    pub(crate) scope_backlink: NativeBytes<[u8; 11]>,
}

pub(crate) struct DirectJointOriginCarrier {
    pub(crate) record_index: u32,
    pub(crate) start: u64,
    pub(crate) length: usize,
    pub(crate) matrix: usize,
    pub(crate) scope_backlink: usize,
    pub(crate) construction_reference: usize,
    pub(crate) reference_suffix: [u8; 4],
    version: u32,
}

impl DirectJointOriginCarrier {
    pub(crate) fn for_scope(scope: &DesignParameterScope) -> Option<[Self; 3]> {
        if scope.kind() != DesignFeatureKind::JointOrigin {
            return None;
        }
        let record_index = scope.record_index.checked_add(1)?;
        if scope.reference_members().values().next() != Some(&record_index) {
            return None;
        }
        let start = scope
            .paired_byte_offset()
            .checked_add(u64_from_index(joint_origin_scope_carrier_separation::LEN))?;
        let short = Self {
            record_index,
            start,
            length: short::LEN,
            matrix: short::MATRIX,
            scope_backlink: short::SCOPE_BACKLINK,
            construction_reference: short::CONSTRUCTION_REFERENCE,
            reference_suffix: short::ZERO_AFTER_REFERENCE_VALUE,
            version: 6,
        };
        let long = Self {
            length: long::LEN,
            matrix: long::MATRIX,
            scope_backlink: long::SCOPE_BACKLINK,
            ..short
        };
        let extended = Self {
            record_index,
            start,
            length: extended::LEN,
            matrix: extended::MATRIX,
            scope_backlink: extended::SCOPE_BACKLINK,
            construction_reference: extended::CONSTRUCTION_REFERENCE,
            reference_suffix: extended::REFERENCE_SUFFIX_VALUE,
            version: 8,
        };
        Some([short, long, extended])
    }

    pub(crate) fn has_registered_types(
        &self,
        scope: &DesignParameterScope,
        types: &HashMap<u64, (&str, u32)>,
    ) -> bool {
        let matches = |record_index, expected_guid: &str, expected_version| {
            types
                .get(&u64::from(record_index))
                .is_some_and(|(guid, version)| {
                    guid.eq_ignore_ascii_case(expected_guid) && *version == expected_version
                })
        };
        matches(scope.record_index, SCOPE_TYPE_GUID, 2)
            && matches(self.record_index, PLACEMENT_TYPE_GUID, self.version)
    }

    pub(crate) fn construction_index(
        &self,
        prefix: &[u8],
        scope_backlink: &[u8; 11],
        scope_record_index: u32,
    ) -> Option<u32> {
        let reference_offset = self.construction_reference - short::ZERO_PREFIX;
        let suffix_offset = reference_offset + scope_backlink.len();
        if prefix
            .get(..reference_offset)?
            .iter()
            .any(|byte| *byte != 0)
            || prefix.get(suffix_offset..)? != self.reference_suffix
            || local_reference_target(scope_backlink, 0, false) != Some(scope_record_index)
        {
            return None;
        }
        local_reference_target(prefix, reference_offset, false).filter(|index| *index != 0)
    }

    pub(crate) fn matches_offsets(&self, frame: &DesignJointOriginTransform) -> bool {
        self.start.checked_add(u64_from_index(self.matrix))
            == Some(frame.joint_origin_transform_offset)
            && frame.reference.as_ref().is_some_and(|reference| {
                reference.joint_origin_reference != 0
                    && self
                        .start
                        .checked_add(u64_from_index(self.construction_reference + 1))
                        == Some(reference.joint_origin_reference_offset)
            })
    }
}

// SPDX-License-Identifier: Apache-2.0
//! Emboss selection links, scalar owners, and native control lanes.

use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::identity::Sourced;
use crate::records::topology::sketch_profile::DesignSketchProfileOperand;
use cadmpeg_core::decode::u64_from_index;
use serde::{Deserialize, Serialize};

cadmpeg_core::named_optional_field!(
    deserialize_emboss_sketch_profile,
    DesignSketchProfileOperand,
    "sketch_profile"
);

/// Fixed-prefix construction carried by an Emboss scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignEmbossConstruction {
    pub(crate) control_words: [u32; 2],
    pub(crate) native_pair: [u64; 2],
    pub(crate) depth_owner: Sourced<u32>,
    pub(crate) auxiliary_group: Sourced<u32>,
    pub(crate) horizontal_owner: Sourced<u32>,
    pub(crate) rotation_owner: Sourced<u32>,
    pub(crate) face_group: Sourced<u32>,
    pub(crate) vertical_owner: Sourced<u32>,
    pub(crate) profile_group: Sourced<u32>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_emboss_sketch_profile"
    )]
    pub(crate) sketch_profile: Option<DesignSketchProfileOperand>,
}

impl DesignEmbossConstruction {
    pub(crate) fn scalar_owners(&self) -> [(Sourced<u32>, &'static str); 4] {
        [
            (self.depth_owner, "Depth"),
            (self.horizontal_owner, "Horizontal Distance"),
            (self.vertical_owner, "Vertical Distance"),
            (self.rotation_owner, "Rotation Angle"),
        ]
    }

    /// Whether the construction agrees with the reference table of `scope`,
    /// and, for a located scope, with its frame offsets.
    pub(crate) fn links_match(&self, scope: &DesignParameterScope) -> bool {
        if scope.kind() != DesignFeatureKind::Emboss || scope.reference_members().len() != 9 {
            return false;
        }
        if scope
            .reference_members()
            .values()
            .enumerate()
            .any(|(ordinal, value)| {
                scope
                    .reference_members()
                    .values()
                    .take(ordinal)
                    .any(|prior| prior == value)
            })
        {
            return false;
        }
        let fields = [
            (self.depth_owner, 4, 29),
            (self.auxiliary_group, 8, 40),
            (self.horizontal_owner, 5, 51),
            (self.rotation_owner, 7, 82),
            (self.face_group, 2, 93),
            (self.vertical_owner, 6, 104),
            (self.profile_group, 0, 119),
        ];
        if self.sketch_profile.as_ref().is_some_and(|profile| {
            profile.scope_reference_ordinal != 1
                || scope.reference_members().values().nth(1) != Some(&profile.record_index)
        }) {
            return false;
        }
        let content = fields
            .iter()
            .enumerate()
            .all(|(ordinal, (field, table, _))| {
                field.value != 0
                    && scope.reference_members().values().nth(*table) == Some(&field.value)
                    && fields[..ordinal]
                        .iter()
                        .all(|(prior, _, _)| prior.value != field.value)
            });
        let (Some(start), Some(count)) = (scope.byte_offset(), scope.reference_count_offset())
        else {
            return content && fields.iter().all(|(field, _, _)| field.offset.is_none());
        };
        content
            && start.checked_add(235) == Some(count)
            && scope
                .reference_members()
                .offsets()
                .enumerate()
                .all(|(ordinal, offset)| {
                    count.checked_add(5 + 11 * u64_from_index(ordinal)) == Some(*offset)
                })
            && fields
                .iter()
                .enumerate()
                .all(|(ordinal, (field, _, relative))| {
                    field.offset.is_some_and(|offset| {
                        start.checked_add(*relative) == Some(offset)
                            && start < offset
                            && offset < count
                            && (ordinal == 0 || fields[ordinal - 1].0.offset < Some(offset))
                    })
                })
    }
}

rewrite_native_record!(DesignEmbossConstruction, []; {control_words, native_pair, depth_owner, auxiliary_group, horizontal_owner, rotation_owner, face_group, vertical_owner, profile_group, sketch_profile});

use crate::records::identity::Located;
use cadmpeg_ir::scalar::FiniteReal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignUnfoldOperation {
    pub(crate) control: Located<u8>,
    pub(crate) prefix_end: u64,
    pub(crate) bend_group: Located<u32>,
    pub(crate) settings: Located<u32>,
    pub(crate) wrapper: Located<u32>,
    pub(crate) stationary_group: Located<u32>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_rule_record_index"
    )]
    pub(crate) rule_record_index: Option<u32>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_rule_material_scalar"
    )]
    pub(crate) rule_material_scalar: Option<Located<FiniteReal>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignRefoldOperation {
    pub(crate) group_record_index: u32,
    pub(crate) group_byte_offset: u64,
    pub(crate) group_paired_byte_offset: u64,
    pub(crate) members: Vec<Located<u32>>,
    pub(crate) preceding_unfolds: Vec<u32>,
}

cadmpeg_core::named_optional_field!(deserialize_rule_record_index, u32, "rule_record_index");
cadmpeg_core::named_optional_field!(
    deserialize_rule_material_scalar,
    Located<FiniteReal>,
    "rule_material_scalar"
);

rewrite_native_scalar!(DesignUnfoldOperation);
rewrite_native_scalar!(DesignRefoldOperation);

impl DesignUnfoldOperation {
    pub(crate) fn valid_for_scope(
        &self,
        scope: &super::super::scope::DesignParameterScope,
    ) -> bool {
        let fields = [
            &self.bend_group,
            &self.settings,
            &self.wrapper,
            &self.stationary_group,
        ];
        let flag_offset = self.control.offset.checked_sub(scope.byte_offset());
        flag_offset.is_some_and(|offset| {
            [
                cadmpeg_core::decode::u64_from_index(
                    crate::layout::sheet_metal_fold_scope_prefix::EXTENSION + 1,
                ),
                cadmpeg_core::decode::u64_from_index(
                    crate::layout::sheet_metal_fold_scope_prefix::EXTENSION + 5,
                ),
            ]
            .contains(&offset)
        }) && self.control.offset.checked_add(1) == Some(self.bend_group.offset)
            && fields
                .windows(2)
                .all(|pair| matches!(pair[1].offset.checked_sub(pair[0].offset), Some(11 | 51)))
            && matches!(
                self.prefix_end.checked_sub(self.stationary_group.offset),
                Some(11 | 51)
            )
            && self.prefix_end <= scope.reference_count_offset()
            && [
                self.bend_group.value,
                self.settings.value,
                self.stationary_group.value,
            ]
            .iter()
            .all(|index| {
                scope
                    .reference_members()
                    .values()
                    .any(|member| member == index)
            })
            && (self.rule_material_scalar.is_none() || self.rule_record_index.is_some())
    }
}

impl DesignRefoldOperation {
    pub(crate) fn valid_for_scope(
        &self,
        scope: &super::super::scope::DesignParameterScope,
    ) -> bool {
        let Some(position) = self
            .members
            .iter()
            .position(|member| member.value == scope.record_index)
        else {
            return false;
        };
        self.members
            .iter()
            .filter(|member| member.value == scope.record_index)
            .count()
            == 1
            && self.members.first().is_some_and(|member| {
                self.group_byte_offset
                    .checked_add(cadmpeg_core::decode::u64_from_index(
                        crate::layout::sheet_metal_unfold_refold_group::FIRST_MEMBER,
                    ))
                    == Some(member.offset)
            })
            && self.members.last().is_some_and(|member| {
                matches!(
                    self.group_paired_byte_offset.checked_sub(member.offset),
                    Some(16 | 56)
                )
            })
            && self
                .members
                .windows(2)
                .all(|pair| matches!(pair[1].offset.checked_sub(pair[0].offset), Some(11 | 51)))
            && self.preceding_unfolds.iter().all(|source| {
                self.members
                    .iter()
                    .take(position)
                    .any(|member| member.value == *source)
            })
            && self
                .preceding_unfolds
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len()
                == self.preceding_unfolds.len()
    }
}

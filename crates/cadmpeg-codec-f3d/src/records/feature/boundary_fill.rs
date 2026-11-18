// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::native::bytes::NativeBytes;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct DesignBoundaryFillScope {
    pub(crate) prefix: Option<DesignBoundaryFill>,
    pub(crate) named_tail: Option<super::scope::DesignNamedScopeReferences>,
}

rewrite_native_record!(DesignBoundaryFillScope, []; { prefix, named_tail });

type LocalReferences = Vec<Option<NonZeroU32>>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignBoundaryFill {
    pub(crate) operation_word: u32,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_operands"
    )]
    pub(crate) operands: Option<DesignBoundaryFillOperands>,
}

impl DesignBoundaryFill {
    pub(super) fn fits_prefix(&self, available: u64) -> bool {
        let Some(available) = usize::try_from(available).ok().and_then(|available| {
            available.checked_sub(crate::layout::boundary_fill_scope_prefix_plain::LEN)
        }) else {
            return false;
        };
        self.operands.as_ref().is_none_or(|operands| {
            operands
                .minimum_size()
                .is_some_and(|size| size <= available)
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignBoundaryFillOperands {
    pub(crate) body_groups: LocalReferences,
    pub(crate) plane_tools: LocalReferences,
    pub(crate) settings: NonZeroU32,
    pub(crate) cells: LocalReferences,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_removal"
    )]
    pub(crate) removal: Option<DesignBoundaryFillRemoval>,
    pub(crate) suffix: NativeBytes,
}

impl DesignBoundaryFillOperands {
    fn minimum_size(&self) -> Option<usize> {
        let list_len = |list: &LocalReferences| {
            list.iter().try_fold(4usize, |length, reference| {
                length.checked_add(if reference.is_some() { 11 } else { 1 })
            })
        };
        let mut size = 11usize.checked_add(self.suffix.len())?;
        for list in [&self.body_groups, &self.plane_tools, &self.cells] {
            size = size.checked_add(list_len(list)?)?;
        }
        if let Some(removal) = &self.removal {
            size = size.checked_add(1)?;
            for list in &removal.references {
                size = size.checked_add(list_len(list)?)?;
            }
        }
        Some(size)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignBoundaryFillRemoval {
    pub(crate) references: [LocalReferences; 2],
    pub(crate) remove_tools: bool,
}

cadmpeg_core::named_optional_field!(deserialize_operands, DesignBoundaryFillOperands, "operands");
cadmpeg_core::named_optional_field!(deserialize_removal, DesignBoundaryFillRemoval, "removal");

rewrite_native_scalar!(DesignBoundaryFill);

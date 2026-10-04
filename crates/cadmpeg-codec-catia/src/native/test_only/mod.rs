// SPDX-License-Identifier: Apache-2.0
//! Test-only CATIA native decode/load/store helpers.

use super::CatiaOwnerPacketPayload;

mod test_consolidated;
mod test_legacy;
mod test_links;
mod test_load;
mod test_zero_entity;

impl CatiaOwnerPacketPayload {
    fn final_reference(&self) -> Option<u32> {
        match self {
            Self::FixedNine { references, .. } => references.last().copied(),
            Self::Counted { references, .. } => references.last().copied(),
        }
    }
}

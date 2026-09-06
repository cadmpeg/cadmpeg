// SPDX-License-Identifier: Apache-2.0
//! A construction reference with its resolved target and source position.

use crate::om::reference_index::ReferenceIndexToken;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConstructionReference<B> {
    pub(crate) token: ReferenceIndexToken,
    pub(crate) data_block: B,
    pub(crate) source_offset: u64,
}

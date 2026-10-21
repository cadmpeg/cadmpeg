// SPDX-License-Identifier: Apache-2.0
//! Closed control fields of a GROUP record.

use crate::records::group::GroupSelector;
use serde::{Deserialize, Serialize};

pub(super) fn selector(value: u8) -> Option<GroupSelector> {
    match value {
        2 => Some(GroupSelector::Face),
        4 => Some(GroupSelector::Edge),
        9 => Some(GroupSelector::Mixed),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub(crate) enum GroupReferenceStatus {
    Form0,
    Form1,
}
impl TryFrom<u8> for GroupReferenceStatus {
    type Error = &'static str;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Form0),
            1 => Ok(Self::Form1),
            _ => Err("linked_reference_status: must be 0 or 1"),
        }
    }
}
impl From<GroupReferenceStatus> for u8 {
    fn from(value: GroupReferenceStatus) -> Self {
        match value {
            GroupReferenceStatus::Form0 => 0,
            GroupReferenceStatus::Form1 => 1,
        }
    }
}

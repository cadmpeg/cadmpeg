// SPDX-License-Identifier: Apache-2.0
//! Closed control fields of a GROUP record.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub(crate) enum GroupSelector { Form2, Form4, Form9 }
impl TryFrom<u8> for GroupSelector {
    type Error = &'static str;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value { 2 => Ok(Self::Form2), 4 => Ok(Self::Form4), 9 => Ok(Self::Form9), _ => Err("selector: must be 2, 4, or 9") }
    }
}
impl From<GroupSelector> for u8 {
    fn from(value: GroupSelector) -> Self { match value { GroupSelector::Form2 => 2, GroupSelector::Form4 => 4, GroupSelector::Form9 => 9 } }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub(crate) enum GroupReferenceStatus { Form0, Form1 }
impl TryFrom<u8> for GroupReferenceStatus {
    type Error = &'static str;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value { 0 => Ok(Self::Form0), 1 => Ok(Self::Form1), _ => Err("linked_reference_status: must be 0 or 1") }
    }
}
impl From<GroupReferenceStatus> for u8 {
    fn from(value: GroupReferenceStatus) -> Self { match value { GroupReferenceStatus::Form0 => 0, GroupReferenceStatus::Form1 => 1 } }
}

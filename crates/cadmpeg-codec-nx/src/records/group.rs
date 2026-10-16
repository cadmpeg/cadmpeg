// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub(crate) enum GroupSelector {
    Instance,
    Face,
    Loop,
    Edge,
    Vertex,
    Surface,
    Curve,
    Point,
    Mixed,
    Region,
}

impl TryFrom<u8> for GroupSelector {
    type Error = &'static str;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Ok(match value {
            1 => Self::Instance,
            2 => Self::Face,
            3 => Self::Loop,
            4 => Self::Edge,
            5 => Self::Vertex,
            6 => Self::Surface,
            7 => Self::Curve,
            8 => Self::Point,
            9 => Self::Mixed,
            10 => Self::Region,
            _ => return Err("selector: must be in 1..=10"),
        })
    }
}

impl From<GroupSelector> for u8 {
    fn from(value: GroupSelector) -> Self {
        match value {
            GroupSelector::Instance => 1,
            GroupSelector::Face => 2,
            GroupSelector::Loop => 3,
            GroupSelector::Edge => 4,
            GroupSelector::Vertex => 5,
            GroupSelector::Surface => 6,
            GroupSelector::Curve => 7,
            GroupSelector::Point => 8,
            GroupSelector::Mixed => 9,
            GroupSelector::Region => 10,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::GroupSelector;

    #[test]
    fn group_selector_wire_covers_the_ten_declared_families() {
        for value in 1..=10u8 {
            let selector = GroupSelector::try_from(value).unwrap();
            assert_eq!(u8::from(selector), value);
            assert_eq!(
                serde_json::to_value(selector).unwrap(),
                serde_json::json!(value)
            );
            assert_eq!(
                serde_json::from_value::<GroupSelector>(serde_json::json!(value)).unwrap(),
                selector
            );
        }
        for value in [0u8, 11, 255] {
            assert!(serde_json::from_value::<GroupSelector>(serde_json::json!(value)).is_err());
        }
    }
}

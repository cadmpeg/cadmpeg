// SPDX-License-Identifier: Apache-2.0
//! Root feature-definition schema classes.

use std::cmp::Ordering;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SchemaClass {
    Hole,
    Round,
    Chamfer,
    Cut,
    Protrusion,
    DatumPlane,
    Section,
    Draft,
    Surface,
    SurfaceMerge,
    CoordinateSystem,
    Unknown(UnknownSchemaClass),
}

/// Unclassified code, constructed only by normalizing an encoded integer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct UnknownSchemaClass(u32);

impl From<u32> for SchemaClass {
    fn from(code: u32) -> Self {
        match code {
            911 => Self::Hole,
            913 => Self::Round,
            914 => Self::Chamfer,
            916 => Self::Cut,
            917 => Self::Protrusion,
            923 => Self::DatumPlane,
            926 => Self::Section,
            927 => Self::Draft,
            942 => Self::Surface,
            946 => Self::SurfaceMerge,
            979 => Self::CoordinateSystem,
            _ => Self::Unknown(UnknownSchemaClass(code)),
        }
    }
}

impl SchemaClass {
    pub(crate) fn code(self) -> u32 {
        match self {
            Self::Hole => 911,
            Self::Round => 913,
            Self::Chamfer => 914,
            Self::Cut => 916,
            Self::Protrusion => 917,
            Self::DatumPlane => 923,
            Self::Section => 926,
            Self::Draft => 927,
            Self::Surface => 942,
            Self::SurfaceMerge => 946,
            Self::CoordinateSystem => 979,
            Self::Unknown(UnknownSchemaClass(code)) => code,
        }
    }
}

impl Ord for SchemaClass {
    fn cmp(&self, other: &Self) -> Ordering {
        self.code().cmp(&other.code())
    }
}

impl PartialOrd for SchemaClass {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for SchemaClass {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.code(), formatter)
    }
}

#[cfg(test)]
mod tests {
    use super::SchemaClass;

    #[test]
    fn schema_classes_preserve_code_order_across_unknown_classes() {
        let mut classes = [979, 949, 913, 1104, 0].map(SchemaClass::from);
        classes.sort();
        assert_eq!(classes.map(SchemaClass::code), [0, 913, 949, 979, 1104]);
        assert_eq!(SchemaClass::from(913), SchemaClass::Round);
        assert!(matches!(SchemaClass::from(949), SchemaClass::Unknown(_)));
        assert_eq!(SchemaClass::from(949).to_string(), "949");
    }
}

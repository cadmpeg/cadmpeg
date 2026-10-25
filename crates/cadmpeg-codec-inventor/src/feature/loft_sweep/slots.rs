// SPDX-License-Identifier: Apache-2.0
//! Record types selected by loft and sweep property slots.

use crate::feature::{ClassId, PmDcFeature};
use crate::pmdc::type_id_string;

pub(crate) const SWEEP_CLASS: ClassId = ClassId(cadmpeg_ir::native::bytes::NativeBytes::new([
    0x05, 0xda, 0xba, 0x90, 0xd1, 0x11, 0x38, 0x61, 0x00, 0x08, 0x0b, 0xbd, 0x06, 0x63, 0xdc, 0x09,
]));
pub(crate) const LOFT_CLASS: ClassId = ClassId(cadmpeg_ir::native::bytes::NativeBytes::new([
    0xe0, 0xef, 0x54, 0x2f, 0xd1, 0x11, 0x34, 0x09, 0x00, 0x08, 0x4e, 0xba, 0x32, 0xa3, 0xdc, 0x09,
]));

type SlotRule = (usize, &'static [[u8; 16]]);

const LOFT_SLOTS: &[SlotRule] = &[
    (0, &[super::SECTION_LIST_TYPE]),
    (1, &[super::super::PART_OPERATION_TYPE]),
    (2, &[super::super::BOOLEAN_TYPE]),
    (6, &[super::super::SURFACE_BODY_TYPE]),
    (7, &[super::SECTION_LIST_TYPE]),
    (9, &[super::super::BOOLEAN_TYPE]),
    (10, &[super::LOFT_MODE_TYPE]),
    (13, &[super::super::OBJECT_COLLECTION_TYPE]),
];

const SWEEP_SLOTS: &[SlotRule] = &[
    (0, &[super::super::BOUNDARY_PATCH_TYPE]),
    (
        1,
        &[
            super::super::PROFILE_SELECTION_TYPE,
            super::super::PLATE_PROFILE_SELECTION_TYPE,
        ],
    ),
    (2, &[super::super::PART_OPERATION_TYPE]),
    (3, &[crate::design::PARAMETER_FULL_TYPE]),
    (4, &[super::super::EXTENT_TYPE]),
    (7, &[super::super::FEATURE_DIMENSIONS_TYPE]),
    (8, &[super::SWEEP_MODE_TYPE]),
    (9, &[super::SWEEP_ORIENTATION_TYPE]),
    (10, &[super::SWEEP_SCALING_TYPE]),
    (15, &[super::super::OBJECT_COLLECTION_TYPE]),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Family {
    SolidLoft,
    SurfaceLoft,
    Sweep,
}

impl Family {
    pub(crate) fn native_name(self) -> &'static str {
        match self {
            Self::SolidLoft => "Loft",
            Self::SurfaceLoft => "LoftSurface",
            Self::Sweep => "Sweep",
        }
    }

    pub(crate) fn from_class_id(class: ClassId) -> Option<Self> {
        match class.0.into_inner() {
            [0xe0, 0xef, 0x54, 0x2f, 0xd1, 0x11, 0x34, 0x09, 0x00, 0x08, 0x4e, 0xba, 0x32, 0xa3, 0xdc, 0x09] => {
                Some(Self::SolidLoft)
            }
            [0x80, 0x8b, 0x00, 0xaf, 0xd4, 0x11, 0x40, 0x88, 0x10, 0x00, 0xfc, 0xa5, 0xbb, 0xa6, 0x47, 0xb5] => {
                Some(Self::SurfaceLoft)
            }
            [0x05, 0xda, 0xba, 0x90, 0xd1, 0x11, 0x38, 0x61, 0x00, 0x08, 0x0b, 0xbd, 0x06, 0x63, 0xdc, 0x09] => {
                Some(Self::Sweep)
            }
            _ => None,
        }
    }

    fn rules(self) -> &'static [SlotRule] {
        match self {
            Self::SolidLoft | Self::SurfaceLoft => LOFT_SLOTS,
            Self::Sweep => SWEEP_SLOTS,
        }
    }

    pub(crate) fn valid_slots<'a>(
        self,
        source: &PmDcFeature,
        type_at: impl Fn(u32) -> Option<&'a str>,
    ) -> bool {
        if source.properties.marker() != 2 {
            return false;
        }
        self.rules().iter().all(|(slot, types)| {
            let Some(reference) = source.properties.references().get(*slot) else {
                return true;
            };
            if reference.index() == 0 {
                return true;
            }
            reference
                .record_ordinal()
                .and_then(&type_at)
                .is_some_and(|actual| {
                    types
                        .iter()
                        .any(|expected| actual == type_id_string(*expected))
                })
        })
    }
}

#[cfg(test)]
mod tests;

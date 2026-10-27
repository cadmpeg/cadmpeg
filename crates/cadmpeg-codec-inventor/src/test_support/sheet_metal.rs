// SPDX-License-Identifier: Apache-2.0
use super::test_fixtures::{content_at, parse};
use crate::feature::sheet_metal::{style, SheetMetalProperty as Sheet};
use crate::feature::{
    PmDcFeature, PmDcFeatureEnumFamily as EnumFamily, PmDcFeaturePayload, PmDcFeatureProperty,
    PmDcFeaturePropertyKind as Kind, PmDcFeaturePropertyPayload,
    PmDcFeatureReferenceFamily as RefFamily,
};
use crate::pmdc::{PmDcListMetadata, PmDcReference as Reference, PmDcReferenceList};
use crate::record_identity::{Located, RecordTypeId};
use cadmpeg_ir::{ids::IdentityKey, scalar::Length};

pub(crate) fn reference(index: u32) -> Reference {
    Reference::new(index, false).expect("reference")
}
pub(crate) fn list(indices: &[u32]) -> PmDcReferenceList {
    PmDcReferenceList::new(
        2,
        (!indices.is_empty()).then_some(PmDcListMetadata::U32([1, 0])),
        indices.iter().copied().map(reference).collect(),
    )
    .expect("list")
}
pub(crate) fn feature(ordinal: u32, slots: &[u32]) -> PmDcFeature {
    let header = parse(&content_at(ordinal + 1, 29, &[]), |_, view| {
        crate::pmdc::state_content_header(&mut crate::pmdc::Cursor::new(view), 29)
    })
    .expect("header");
    let payload:PmDcFeaturePayload=serde_json::from_value(serde_json::json!({"save_version_major":29,"header":header,"state":-1,"outline_value":0,"properties":list(slots),"value":0})).expect("feature");
    Located::new(
        payload,
        RecordTypeId::try_from("914d8790d011f8d10008cabc0663dc09".to_owned()).expect("type"),
        IdentityKey::try_new("fixture").expect("token"),
        ordinal,
    )
}
pub(crate) fn property(index: u32, kind: &Kind) -> PmDcFeatureProperty {
    let header = parse(&content_at(index, 29, &[-1, 8]), |_, view| {
        crate::pmdc::content_header(&mut crate::pmdc::Cursor::new(view), 29)
    })
    .expect("header");
    let payload: PmDcFeaturePropertyPayload = serde_json::from_value(
        serde_json::json!({"save_version_major":29,"header":header,"kind":kind}),
    )
    .expect("property");
    Located::new(
        payload,
        if matches!(kind, Kind::ProfileSelection { .. }) {
            RecordTypeId::try_from("473f20fcd11101cf000835bd0663dc09".to_owned()).expect("type")
        } else {
            RecordTypeId::try_from("00000000000000000000000000000001".to_owned()).expect("type")
        },
        IdentityKey::try_new("fixture").expect("token"),
        index - 1,
    )
}
pub(crate) struct Fixture {
    pub(crate) plate: PmDcFeature,
    pub(crate) bend: PmDcFeature,
    pub(crate) style: style::Style,
    pub(crate) properties: Vec<PmDcFeatureProperty>,
}
impl Fixture {
    pub(crate) fn new() -> Self {
        let boolean = |value| Kind::Boolean {
            name: String::new(),
            name_value: 0,
            value,
        };
        let sheet = |definition| Kind::SheetMetal { definition };
        let enumeration = |type_value, value| sheet(Sheet::Enumeration { type_value, value });
        let mut properties = vec![
            property(
                1,
                &Kind::ProfileSelection {
                    entity_link: reference(0),
                    value: 0,
                },
            ),
            property(
                8,
                &sheet(Sheet::Corner {
                    control: reference(70),
                    gap: reference(71),
                    size: reference(72),
                    relief_form: reference(73),
                    corner_form: reference(74),
                    secondary_form: reference(75),
                    jacobi_radius: reference(76),
                    value: 0,
                    option: reference(77),
                }),
            ),
            property(70, &boolean(true)),
            property(73, &enumeration(501, 103)),
            property(74, &enumeration(8, 3)),
            property(75, &enumeration(501, 303)),
            property(77, &boolean(true)),
            property(3, &boolean(false)),
            property(
                5,
                &sheet(Sheet::Attachment {
                    control: reference(20),
                    thickness: reference(4),
                    first_mode: reference(21),
                    directions: [reference(33), reference(34)],
                    radius: reference(25),
                    second_mode: reference(22),
                    relief_width: reference(26),
                    minimum_remnant: reference(28),
                    first_option: reference(23),
                    states: cadmpeg_ir::native::bytes::NativeBytes::new([1, 1, 1]),
                    second_option: reference(24),
                    extension: None,
                }),
            ),
            property(
                6,
                &Kind::References {
                    family: RefFamily::EdgeCollection,
                    items: list(&[]),
                },
            ),
            property(
                7,
                &Kind::References {
                    family: RefFamily::FeatureDimensions,
                    items: list(&[]),
                },
            ),
            property(
                9,
                &sheet(Sheet::Relief {
                    control: reference(29),
                    width: reference(26),
                    depth: reference(27),
                    form: reference(31),
                    minimum_remnant: reference(28),
                    seam_gap: reference(30),
                }),
            ),
            property(
                10,
                &sheet(Sheet::Transition {
                    form: reference(32),
                    control: reference(35),
                    radius: reference(36),
                }),
            ),
            property(12, &sheet(Sheet::PlateControl { value: 0 })),
            property(
                13,
                &Kind::References {
                    family: RefFamily::ObjectCollection,
                    items: list(&[46]),
                },
            ),
            property(
                14,
                &Kind::Enumeration {
                    family: EnumFamily::PartOperation,
                    type_value: 5,
                    value: 3,
                },
            ),
            property(20, &boolean(false)),
            property(21, &enumeration(9, 1)),
            property(22, &enumeration(9, 1)),
            property(23, &boolean(false)),
            property(24, &boolean(false)),
            property(29, &boolean(true)),
            property(31, &enumeration(501, 1)),
            property(32, &enumeration(4, 0)),
            property(35, &boolean(true)),
            property(40, &enumeration(501, 1)),
            property(43, &enumeration(4, 0)),
            property(
                44,
                &sheet(Sheet::BendControl {
                    value: 6,
                    state: Some(1),
                }),
            ),
            property(45, &boolean(true)),
            property(46, &Kind::SurfaceBody { body: reference(0) }),
        ];
        properties.sort_by_key(|p| p.identity.record_ordinal);
        let mut bytes = vec![0; 30];
        for value in [4_u32, 25, 40, 60, 61, 62, 63, 30, 28, 23, 0, 0, 0, 43, 36] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        let payload = parse(&bytes, |ctx, view| style::parse(ctx, view, 29)).expect("style");
        let style = Located::new(
            payload,
            RecordTypeId::from_bytes(style::TYPE),
            IdentityKey::try_new("fixture").expect("token"),
            900,
        );
        Self {
            plate: feature(
                100,
                &[1, 2, 3, 4, 5, 0, 0, 6, 7, 8, 9, 10, 11, 12, 0, 13, 14],
            ),
            bend: feature(200, &[6, 25, 4, 5, 9, 0, 7, 10, 44, 13, 45]),
            style,
            properties,
        }
    }
    pub(crate) fn property(&self, reference: Reference) -> Option<&PmDcFeatureProperty> {
        self.properties
            .iter()
            .find(|p| p.identity.record_ordinal + 1 == reference.index())
    }
    pub(crate) fn length(reference: Reference) -> Option<Length> {
        Some(
            Length::new(match reference.index() {
                4 => 7.0,
                25 => 11.0,
                26 => 13.0,
                27 => 17.0,
                28 => 0.0,
                30 => 19.0,
                36 => 23.0,
                71 | 72 | 76 => 0.0,
                _ => return None,
            })
            .expect("length"),
        )
    }
    pub(crate) fn direction(reference: Reference) -> Option<cadmpeg_ir::units::UnitVector3> {
        use cadmpeg_ir::units::UnitVector3;
        match reference.index() {
            2 => Some(UnitVector3::X_AXIS.reversed()),
            33 => Some(UnitVector3::X_AXIS),
            34 => Some(UnitVector3::Y_AXIS),
            _ => None,
        }
    }
    pub(crate) fn values(&self) -> crate::feature::sheet_metal::operands::PlateOperands {
        crate::feature::sheet_metal::operands::plate(
            &self.plate,
            &self.style,
            |r| self.property(r),
            Self::length,
            Self::direction,
            cadmpeg_ir::units::UnitVector3::Z_AXIS,
            cadmpeg_ir::units::UnitVector3::X_AXIS,
        )
        .expect("operands")
    }
    pub(crate) fn edit(&mut self, index: u32, edit: impl FnOnce(&mut Kind)) {
        let p = self
            .properties
            .iter_mut()
            .find(|p| p.identity.record_ordinal + 1 == index)
            .expect("property");
        let mut payload = (**p).clone();
        edit(&mut payload.kind);
        *p = Located::new(
            payload,
            p.identity.type_id.clone(),
            p.identity.segment_token.clone(),
            p.identity.record_ordinal,
        );
    }
}

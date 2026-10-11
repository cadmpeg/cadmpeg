use super::{
    inventory, parse_boolean, parse_boundary_patch, parse_chamfer, parse_edge_item,
    parse_entity_style_link, parse_feature, parse_fillet_edge_selection, parse_fillet_edge_set,
    parse_label, parse_part_operation, parse_pattern_feature, parse_placement,
    parse_profile_selection, parse_rdx_variable, parse_surface_body, parse_terminator,
    project_chamfer, project_extrusion, project_fillet, project_hole, ClassId, PmDcEntityStyleLink,
    PmDcEntityStyleLinkPayload, PmDcFeature, PmDcFeatureEnumFamily, PmDcFeatureLabel,
    PmDcFeatureLabelPayload, PmDcFeatureLabelPayloadWire, PmDcFeaturePayload, PmDcFeatureProperty,
    PmDcFeaturePropertyKind, PmDcFeaturePropertyPayload, PmDcFeatureReferenceFamily,
    PmDcLinkedHeader, PmDcPatternFamily, ProjectionIndex, BOOLEAN_TYPE, CHAMFER_CLASS_ID,
    END_OF_FEATURES_TYPE, ENTITY_STYLE_LINK_TYPE, EXTRUSION_CLASS_ID, FEATURE_LABEL_TYPE,
    FEATURE_TYPE, FILLET_CLASS_ID, HOLE_CLASS_ID, MIRROR_FEATURE_TYPE,
    RECTANGULAR_PATTERN_FEATURE_TYPE,
};
use crate::container::InventorContainer;
use crate::pmdc::{PmDcContentHeader, PmDcReferenceList, PmDcU32List};
use crate::record_identity::Located;
use crate::rse::{RecordFrameState, SegmentBulkState, SegmentKind};
use crate::test_support::test_fixtures::{content, parse, primary_envelope_fixture};
use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    edge_treatments::{ChamferSpec, RadiusSpec},
    holes::{HoleKind, HolePlacement},
    BooleanOp, DesignParameter, ExtrudeDirection, ExtrudeExtent, ExtrudeSide, Feature,
    FeatureDefinition, FeatureOperation, FeatureResultTopology, LinearTermination,
};
use cadmpeg_ir::features::{ParameterId, ParameterValue};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::{Angle, Length};
use cadmpeg_ir::sketches::Sketch;
use cadmpeg_ir::sketches::{SketchId, SketchPlacement};
use std::collections::BTreeMap;

use super::{
    admit_projected_feature, boolean_properties, closed_edge_items, feature_result, project,
    FeatureInventory,
};

mod native_serialization;
mod parsing;
mod projection;
mod token_tests;

fn inventory_with_record(
    type_id: [u8; 16],
    payload: &[u8],
    policy: DecodePolicy,
) -> Result<super::FeatureInventory, CodecError> {
    let bytes = primary_envelope_fixture();
    let payload = payload.to_vec();
    let arena = DecodeArena::new();
    let (setup_ctx, source) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("envelope view");
    let mut container = InventorContainer::open(&setup_ctx, source).expect("framed envelope");
    let segment = &mut container.rse.segments[0];
    segment.kind = SegmentKind::PmDc;
    let SegmentBulkState::Framed(bulk) = &mut segment.bulk else {
        panic!("framed bulk fixture");
    };
    let RecordFrameState::Framed(table) = &mut bulk.records else {
        panic!("framed record fixture");
    };
    table.records[0].type_id = type_id;
    table.records[0].payload = View::over_retained(&payload);
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("input view");
    inventory(&ctx, &container.rse)
}

pub(super) fn segment() -> cadmpeg_ir::ids::IdentityKey {
    cadmpeg_ir::identity_key!("generated")
}

fn references(values: &[u32]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&0x3000u16.to_le_bytes());
    bytes.extend_from_slice(
        &(u32::try_from(values.len()).expect("fixture value fits u32")).to_le_bytes(),
    );
    if !values.is_empty() {
        bytes.extend_from_slice(
            &(u32::try_from(values.len()).expect("fixture value fits u32")).to_le_bytes(),
        );
        bytes.extend_from_slice(&0u32.to_le_bytes());
    }
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

fn utf16(value: &str) -> Vec<u8> {
    let units = value.encode_utf16().collect::<Vec<_>>();
    let mut bytes = (u32::try_from(units.len()).expect("fixture value fits u32"))
        .to_le_bytes()
        .to_vec();
    for unit in units {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes
}

fn reference(index: u32) -> crate::pmdc::PmDcReference {
    crate::pmdc::PmDcReference::new(index, index != 0).expect("test reference index fits 31 bits")
}

fn reference_list(values: &[u32]) -> PmDcReferenceList {
    PmDcReferenceList::new(
        2,
        (!values.is_empty()).then_some(crate::pmdc::PmDcListMetadata::U32([
            u32::try_from(values.len()).expect("fixture value fits u32"),
            0,
        ])),
        values.iter().copied().map(reference).collect(),
    )
    .expect("test list metadata matches length")
}

fn test_header() -> PmDcContentHeader {
    PmDcContentHeader {
        header_value: 0,
        header_id: 0,
        next: reference(0),
        flags: 0,
        context: reference(0),
        source_index: 0,
    }
}

fn test_property(ordinal: u32, kind: PmDcFeaturePropertyKind) -> PmDcFeatureProperty {
    Located::new(
        PmDcFeaturePropertyPayload {
            save_version_major: 16,
            header: test_header(),
            kind,
        },
        crate::record_identity::RecordTypeId::try_from(format!("{ordinal:032x}"))
            .expect("test GUID"),
        segment()
            .try_clone_for_decode(
                &cadmpeg_test_support::service_decode_context(),
                "Inventor located fixture token",
            )
            .expect("service fixture token"),
        ordinal,
    )
}

pub(super) fn test_feature(ordinal: u32, slot_count: usize, slots: &[(usize, u32)]) -> PmDcFeature {
    let mut references = vec![reference(0); slot_count];
    for (slot, record_ordinal) in slots {
        references[*slot] = reference(record_ordinal + 1);
    }
    Located::new(
        PmDcFeaturePayload {
            save_version_major: 16,
            header: test_header(),
            state: 69,
            outline_value: 0,
            properties: PmDcReferenceList::new(
                2,
                (!references.is_empty()).then_some(crate::pmdc::PmDcListMetadata::U32([
                    u32::try_from(slot_count).expect("fixture value fits u32"),
                    0,
                ])),
                references,
            )
            .expect("test list metadata matches length"),
            value: 0,
        },
        test_type_id(FEATURE_TYPE),
        segment()
            .try_clone_for_decode(
                &cadmpeg_test_support::service_decode_context(),
                "Inventor located fixture token",
            )
            .expect("service fixture token"),
        ordinal,
    )
}

pub(super) fn test_type_id(value: [u8; 16]) -> crate::record_identity::RecordTypeId {
    let ctx = cadmpeg_test_support::service_decode_context();
    crate::record_identity::RecordTypeId::from_bytes(
        &ctx,
        value,
        "retain Inventor PmDc test record type id",
    )
    .expect("service fixture type id")
}

fn decode_label(value: serde_json::Value) -> Result<PmDcFeatureLabel, String> {
    let ctx = cadmpeg_test_support::service_decode_context();
    let wire = serde_json::from_value::<
        crate::record_identity::LocatedWire<PmDcFeatureLabelPayloadWire>,
    >(value)
    .map_err(|error| error.to_string())?;
    crate::record_identity::LocatedWire {
        id: wire.id,
        type_id: wire.type_id,
        segment_token: wire.segment_token,
        record_ordinal: wire.record_ordinal,
        value: wire
            .value
            .into_record(&ctx)
            .map_err(|error| error.to_string())?,
    }
    .into_record(&ctx)
    .map_err(|error| error.to_string())
}

pub(super) fn test_label(
    owner_ordinal: u32,
    index: u32,
    class_id: ClassId,
    participants: &[u32],
) -> PmDcFeatureLabel {
    let ctx = cadmpeg_test_support::service_decode_context();
    let payload = PmDcFeatureLabelPayloadWire::<String> {
        save_version_major: 16,
        header: PmDcLinkedHeader {
            header_value: 0,
            header_id: 0,
            values: [0; 2],
            owner: reference(owner_ordinal + 1),
            parent: reference(0),
            next: reference(0),
        },
        index,
        participants: reference_list(participants),
        name: format!("Feature {index}"),
        class_id: class_id
            .into_text(&ctx, "retain Inventor feature label fixture class id")
            .expect("service fixture class id"),
    }
    .into_record(&ctx)
    .expect("valid label fixture");
    Located::new(
        payload,
        test_type_id(FEATURE_LABEL_TYPE),
        segment()
            .try_clone_for_decode(&ctx, "Inventor located fixture token")
            .expect("service fixture token"),
        owner_ordinal + 1000,
    )
}

fn raw_parameter(ordinal: u32) -> crate::design::PmDcParameter {
    Located::new(
        crate::design::PmDcParameterPayload {
            save_version_major: 16,
            header: crate::pmdc::PmDcContentHeader {
                header_value: 0,
                header_id: 0,
                next: reference(0),
                flags: 0,
                context: reference(0),
                source_index: ordinal,
            },
            name: format!("p{ordinal}"),
            name_value: 0,
            unit: reference(0),
            formula: reference(0),
            nominal_value: cadmpeg_ir::scalar::FiniteReal::ZERO,
            model_value: cadmpeg_ir::scalar::FiniteReal::ZERO,
            tolerance: 0,
            terminal_value: 0,
        },
        crate::record_identity::RecordTypeId::try_from(
            "264d8790d011f8d10008cabc0663dc09".to_owned(),
        )
        .expect("test GUID"),
        segment()
            .try_clone_for_decode(
                &cadmpeg_test_support::service_decode_context(),
                "Inventor located fixture token",
            )
            .expect("service fixture token"),
        ordinal,
    )
}

fn neutral_parameter(raw: &crate::design::PmDcParameter, value: ParameterValue) -> DesignParameter {
    let ctx = cadmpeg_test_support::service_decode_context();
    DesignParameter {
        id: ParameterId::mint(format!(
            "inventor:design:parameter#{}",
            raw.identity.record_ordinal
        ))
        .expect("identity grammar"),
        owner: None,
        ordinal: raw.identity.record_ordinal,
        name: raw.name.clone(),
        expression: String::new(),
        display: None,
        value: Some(value),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: Some(raw.id(&ctx).expect("service fixture record identity")),
    }
}

// The fixture builder keeps each independently indexed record family explicit.
#[allow(clippy::too_many_arguments)]
fn test_projection_index<'a>(
    properties: &'a [PmDcFeatureProperty],
    parameters: &'a [crate::design::PmDcParameter],
    neutral_parameters: &'a [DesignParameter],
    sketches: &'a [crate::sketch::PmDcSketch],
    neutral_sketches: &'a [Sketch],
    directions: &'a [crate::sketch::PmDcDirection],
    transforms: &'a [crate::sketch::PmDcTransform],
    entity_style_links: &'a [PmDcEntityStyleLink],
) -> ProjectionIndex<'a> {
    ProjectionIndex {
        properties: properties
            .iter()
            .map(|record| {
                (
                    (
                        record.identity.segment_token.as_str(),
                        record.identity.record_ordinal,
                    ),
                    Some(record),
                )
            })
            .collect(),
        parameters: parameters
            .iter()
            .map(|record| {
                (
                    (
                        record.identity.segment_token.as_str(),
                        record.identity.record_ordinal,
                    ),
                    Some(record),
                )
            })
            .collect(),
        parameter_values: neutral_parameters
            .iter()
            .filter_map(|parameter| {
                Some((parameter.native_ref.as_deref()?, parameter.value.as_ref()?))
            })
            .collect(),
        sketches: sketches
            .iter()
            .map(|record| {
                (
                    (
                        record.identity.segment_token.as_str(),
                        record.identity.record_ordinal,
                    ),
                    Some(record),
                )
            })
            .collect(),
        sketch_ids: neutral_sketches
            .iter()
            .filter_map(|sketch| {
                sketch
                    .native_ref
                    .as_deref()
                    .map(|native| (native, &sketch.id))
            })
            .collect(),
        directions: directions
            .iter()
            .map(|record| {
                (
                    (
                        record.identity.segment_token.as_str(),
                        record.identity.record_ordinal,
                    ),
                    Some(record),
                )
            })
            .collect(),
        transforms: transforms
            .iter()
            .map(|record| {
                (
                    (
                        record.identity.segment_token.as_str(),
                        record.identity.record_ordinal,
                    ),
                    Some(record),
                )
            })
            .collect(),
        entity_style_links: entity_style_links
            .iter()
            .map(|record| {
                (
                    record.identity.segment_token.as_str(),
                    record.identity.record_ordinal,
                )
            })
            .collect(),
    }
}

pub(super) fn pattern_feature_bytes(version: u8, family: PmDcPatternFamily) -> Vec<u8> {
    let mut bytes = content(21);
    bytes.extend_from_slice(&69u32.to_le_bytes());
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(&references(&[]));
    bytes.extend_from_slice(&7u32.to_le_bytes());
    bytes.extend_from_slice(&references(&[0x8000_0010, 0x8000_0011]));
    for index in 0..6 {
        bytes.extend_from_slice(&(0x8000_0020u32 + index).to_le_bytes());
    }
    bytes.push(1);
    match family {
        PmDcPatternFamily::Rectangular => {
            let remaining = if version > 20 { 26 } else { 20 };
            for index in 0..remaining {
                bytes.extend_from_slice(&(0x8000_0040u32 + index).to_le_bytes());
            }
        }
        PmDcPatternFamily::Mirror => {
            for index in 0..5 {
                bytes.extend_from_slice(&(0x8000_0040u32 + index).to_le_bytes());
            }
            if version > 20 {
                for index in 0..6 {
                    bytes.extend_from_slice(&(0x100u32 + index).to_le_bytes());
                }
            }
            for index in 0..2 {
                bytes.extend_from_slice(&(0x8000_0050u32 + index).to_le_bytes());
            }
        }
    }
    bytes
}

fn feature_label_bytes() -> Vec<u8> {
    let mut label = Vec::new();
    label.extend_from_slice(&0u32.to_le_bytes());
    label.extend_from_slice(&18u16.to_le_bytes());
    label.extend_from_slice(&[0; 20]);
    label.extend_from_slice(&0u32.to_le_bytes());
    label.extend(references(&[]));
    label.extend(utf16("a"));
    label.extend_from_slice(&[0xab; 16]);
    label
}

type FeatureProjection = Option<Result<(Feature, FeatureResultTopology), CodecError>>;

fn generated_extrusion(selections: &[u32], policy: DecodePolicy) -> FeatureProjection {
    let (projection, _, session) = generated_extrusion_with_work(selections, policy, false);
    if projection.is_none() {
        assert!(
            session.is_ok(),
            "a skipped projection leaves the session clean"
        );
    }
    projection
}

fn generated_extrusion_with_work(
    selections: &[u32],
    policy: DecodePolicy,
    measure_work: bool,
) -> (FeatureProjection, Option<u64>, Result<(), CodecError>) {
    let raw_length = raw_parameter(70);
    let raw_taper = raw_parameter(71);
    let neutral_parameters = vec![
        neutral_parameter(
            &raw_length,
            ParameterValue::Length(Length::new(12.0).expect("finite length fixture")),
        ),
        neutral_parameter(
            &raw_taper,
            ParameterValue::Angle(Angle::new(0.1).expect("finite angle fixture")),
        ),
    ];
    let raw_sketch = Located::new(
        crate::sketch::PmDcSketchPayload {
            save_version_major: 16,
            header: test_header(),
            state: 0,
            count_value: 0,
            entities: PmDcReferenceList::new(8, None, Vec::new()).expect("empty entity list"),
            transform: reference(0),
            direction: reference(0),
            values: [0; 2],
            auxiliary: None,
        },
        crate::record_identity::RecordTypeId::try_from(
            "114d8790d011f8d10008cabc0663dc09".to_owned(),
        )
        .expect("test GUID"),
        segment()
            .try_clone_for_decode(
                &cadmpeg_test_support::service_decode_context(),
                "Inventor located fixture token",
            )
            .expect("service fixture token"),
        50,
    );
    let neutral_sketch = Sketch {
        id: SketchId::mint(format!("inventor:design:sketch#{}-50", segment()))
            .expect("valid test fixture"),
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("valid test fixture"),
        profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
        native_ref: Some(
            raw_sketch
                .id(&cadmpeg_test_support::service_decode_context())
                .expect("service fixture record identity"),
        ),
    };
    let direction = Located::new(
        crate::sketch::PmDcDirectionPayload {
            save_version_major: 16,
            header: test_header(),
            entity_flags: 0,
            parameter: cadmpeg_ir::scalar::FiniteReal::ZERO,
            extension: None,
            direction: [
                cadmpeg_ir::scalar::FiniteReal::ZERO,
                cadmpeg_ir::scalar::FiniteReal::ZERO,
                cadmpeg_ir::scalar::FiniteReal::ONE,
            ],
        },
        crate::record_identity::RecordTypeId::try_from(
            "40df52ced011d0d20008ccbc0663dc09".to_owned(),
        )
        .expect("test GUID"),
        segment()
            .try_clone_for_decode(
                &cadmpeg_test_support::service_decode_context(),
                "Inventor located fixture token",
            )
            .expect("service fixture token"),
        60,
    );
    let entity_link = Located::new(
        PmDcEntityStyleLinkPayload {
            save_version_major: 16,
            header: PmDcLinkedHeader {
                header_value: 0,
                header_id: 0,
                values: [0; 2],
                owner: reference(0),
                parent: reference(0),
                next: reference(0),
            },
            value: 0,
            associative_id: 1,
            entity_type: 1,
        },
        test_type_id(ENTITY_STYLE_LINK_TYPE),
        segment()
            .try_clone_for_decode(
                &cadmpeg_test_support::service_decode_context(),
                "Inventor located fixture token",
            )
            .expect("service fixture token"),
        51,
    );
    let properties = vec![
        test_property(
            1,
            PmDcFeaturePropertyKind::Enumeration {
                family: PmDcFeatureEnumFamily::PartOperation,
                type_value: 5,
                value: 1,
            },
        ),
        test_property(
            2,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::BoundaryPatch,
                items: reference_list(selections),
            },
        ),
        test_property(
            3,
            PmDcFeaturePropertyKind::ProfileSelection {
                entity_link: reference(52),
                value: 0,
            },
        ),
        test_property(
            4,
            PmDcFeaturePropertyKind::Boolean {
                name: String::new(),
                name_value: 0,
                value: true,
            },
        ),
        test_property(
            5,
            PmDcFeaturePropertyKind::Enumeration {
                family: PmDcFeatureEnumFamily::Extent,
                type_value: 11,
                value: 1,
            },
        ),
        test_property(
            6,
            PmDcFeaturePropertyKind::Boolean {
                name: String::new(),
                name_value: 0,
                value: false,
            },
        ),
        test_property(
            7,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::ObjectCollection,
                items: reference_list(&[9]),
            },
        ),
        test_property(
            8,
            PmDcFeaturePropertyKind::SurfaceBody {
                body: reference(30),
            },
        ),
    ];
    let feature = test_feature(
        100,
        27,
        &[
            (0, 1),
            (1, 2),
            (2, 60),
            (3, 4),
            (4, 70),
            (5, 71),
            (6, 5),
            (7, 6),
            (23, 2),
            (26, 7),
        ],
    );
    let label = test_label(100, 5, EXTRUSION_CLASS_ID, &[51]);
    let raw_parameters = vec![raw_length, raw_taper];
    let index = test_projection_index(
        &properties,
        &raw_parameters,
        &neutral_parameters,
        std::slice::from_ref(&raw_sketch),
        std::slice::from_ref(&neutral_sketch),
        std::slice::from_ref(&direction),
        &[],
        std::slice::from_ref(&entity_link),
    );
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("projection context");
    let projection = project_extrusion(&ctx, &feature, &label, &index);
    let used = measure_work.then(|| {
        let refusal = ctx
            .charge_work_limit(
                policy.limits.max_work_units,
                "measure Inventor extrusion prefix",
            )
            .expect_err("the probe exceeds the remaining work allowance");
        assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
        refusal.used
    });
    let session = ctx.finish_session();
    (projection, used, session)
}

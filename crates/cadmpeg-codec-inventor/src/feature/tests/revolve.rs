// SPDX-License-Identifier: Apache-2.0
//! Revolves and the work axes they turn about.

use super::*;
use crate::feature::{
    parse_work_axis, project_revolve, FeatureFamily, REVOLVE_CLASS_ID, REVOLVE_EXTENT_FULL,
};

/// A revolve of one profile selection about a work axis through
/// (0.5, 0, 0) cm along the sketch v axis, with extent value `extent`.
fn generated_revolve(extent: u16) -> Option<Result<(Feature, FeatureResultTopology), CodecError>> {
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
        segment().clone(),
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
        native_ref: Some(raw_sketch.id()),
    };
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
                flags_extension: None,
            },
            value: 0,
            associative_id: 1,
            entity_type: 1,
        },
        crate::record_identity::RecordTypeId::from_bytes(ENTITY_STYLE_LINK_TYPE),
        segment().clone(),
        51,
    );
    let real = |value: f64| cadmpeg_ir::scalar::FiniteReal::new(value).expect("finite");
    let properties = vec![
        test_property(
            1,
            PmDcFeaturePropertyKind::Enumeration {
                family: PmDcFeatureEnumFamily::PartOperation,
                type_value: 5,
                value: 2,
            },
        ),
        test_property(
            2,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::BoundaryPatch,
                items: reference_list(&[4]),
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
            PmDcFeaturePropertyKind::WorkAxis {
                entity_flags: 0,
                values: vec![0, 0],
                references: reference_list(&[]),
                marker: false,
                origin: [real(0.5), real(0.0), real(0.0)],
                span: [real(0.0), real(2.0), real(0.0)],
                state: 1,
            },
        ),
        test_property(
            5,
            PmDcFeaturePropertyKind::Enumeration {
                family: PmDcFeatureEnumFamily::Extent,
                type_value: 11,
                value: extent,
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
    // Slot 1 names property record 2 and slot 2 names property record 4.
    let feature = test_feature(101, 20, &[(0, 1), (1, 2), (2, 4), (3, 5), (17, 7)]);
    let label = test_label(101, 6, REVOLVE_CLASS_ID, &[51]);
    let index = test_projection_index(
        &properties,
        &[],
        &[],
        std::slice::from_ref(&raw_sketch),
        std::slice::from_ref(&neutral_sketch),
        &[],
        &[],
        std::slice::from_ref(&entity_link),
    );
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("projection context");
    project_revolve(&ctx, &feature, &label, &index)
}

#[test]
fn projects_a_full_revolve_about_its_work_axis() {
    assert!(matches!(
        FeatureFamily::from_class_id(REVOLVE_CLASS_ID),
        Some(FeatureFamily::Revolve)
    ));
    let (projected, _) = generated_revolve(REVOLVE_EXTENT_FULL)
        .expect("revolve candidate")
        .expect("revolve projection");
    let FeatureDefinition::Operation(FeatureOperation::Revolve {
        construction:
            cadmpeg_ir::features::RevolveConstruction::Resolved {
                axis,
                extent:
                    cadmpeg_ir::features::RevolveExtent::OneSided {
                        termination: cadmpeg_ir::features::AngularTermination::Angle { angle },
                    },
                ..
            },
        op: BooleanOp::Cut,
    }) = projected.evaluation.definition()
    else {
        panic!("full revolve");
    };
    assert_eq!(axis.origin.get(), Point3::new(5.0, 0.0, 0.0));
    assert_eq!(axis.direction.get(), Vector3::new(0.0, 1.0, 0.0));
    assert_eq!(angle.get(), std::f64::consts::TAU);
    // Other extents stay native.
    assert!(generated_revolve(1).is_none());
}

#[test]
fn parses_generated_work_axis_records() {
    let axis = |marker: bool| {
        let mut bytes = content(5);
        bytes.extend_from_slice(&0x0042_4000u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 8]);
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&0x3000u16.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        if marker {
            bytes.extend_from_slice(&0x203u32.to_le_bytes());
        }
        for value in [-1.0f64, 0.0, 0.0, 2.0, 0.0, 0.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.push(0);
        bytes
    };
    for marker in [false, true] {
        let bytes = axis(marker);
        assert_eq!(bytes.len(), if marker { 95 } else { 91 });
        let parsed = parse(&bytes, |ctx, source| {
            parse_work_axis(ctx, source, 19).expect("work axis")
        });
        assert!(matches!(
            parsed.kind,
            PmDcFeaturePropertyKind::WorkAxis { entity_flags: 0x0042_4000, marker: parsed_marker, origin, span, state: 0, .. }
                if parsed_marker == marker
                    && origin[0].get() == -1.0
                    && span[0].get() == 2.0
        ));
    }
}

#[test]
fn parses_a_version_25_work_axis_after_its_stamps_and_third_value() {
    let mut bytes = content_at(5, 25, &[-1, 2512]);
    bytes.extend_from_slice(&0x0042_4000u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 12]);
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&0x3000u16.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    for value in [0.0f64, 0.0, -1.0, 0.0, 0.0, 2.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.push(0);
    let parsed = parse(&bytes, |ctx, source| {
        parse_work_axis(ctx, source, 25).expect("work axis")
    });
    assert_eq!(parsed.header.stamps, [-1, 2512]);
    let PmDcFeaturePropertyKind::WorkAxis {
        values,
        marker,
        origin,
        span,
        ..
    } = parsed.kind
    else {
        panic!("work axis");
    };
    assert_eq!(values.len(), 3);
    assert!(!marker);
    assert_eq!(origin[2].get(), -1.0);
    assert_eq!(span[2].get(), 2.0);
}

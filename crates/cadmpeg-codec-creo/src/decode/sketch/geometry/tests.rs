// SPDX-License-Identifier: Apache-2.0

use super::{saved_section_arc, saved_section_arc_carrier, section_arc_geometry, BTreeMap};
use crate::feature::definitions::{FeatureSegment, FeatureSegmentKind};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::scalar::{Angle, Length};
use cadmpeg_ir::sketches::{SketchGeometry, SketchGeometryDefinition, SketchId};

mod missing_line;

fn saved_profile_fixture() -> (SketchId, Vec<(u32, SketchGeometry)>) {
    let sketch = SketchId::mint("creo:model:sketch#917").expect("sketch identity");
    let mut geometries = Vec::new();
    geometries.push((
        30,
        SketchGeometry::try_from(SketchGeometryDefinition::Arc {
            center: Point2::new(8.0, 8.0),
            radius: Length::new(2.0).expect("radius"),
            start_angle: Angle::new(0.0).expect("start angle"),
            end_angle: Angle::new(std::f64::consts::TAU).expect("end angle"),
        })
        .expect("circle"),
    ));
    for (index, (start, end)) in [
        ([0.0, 0.0], [1.0, 0.0]),
        ([1.0, 0.0], [1.0, 1.0]),
        ([1.0, 1.0], [0.0, 1.0]),
        ([0.0, 1.0], [0.0, 0.0]),
    ]
    .into_iter()
    .enumerate()
    {
        geometries.push((
            10 + u32::try_from(index).expect("fixture value fits u32"),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(start[0], start[1]),
                end: Point2::new(end[0], end[1]),
            })
            .expect("line"),
        ));
    }
    (sketch, geometries)
}

#[test]
fn saved_profile_collections_preserve_circle_and_line_order() {
    let (sketch, geometries) = saved_profile_fixture();
    let profiles = crate::decode::with_test_decode_ctx(|ctx| {
        super::saved_profile_chains(ctx, &sketch, &geometries)
    })
    .expect("admitted profiles");
    assert_eq!(profiles.len(), 2);
    assert_eq!(
        profiles[0][0].entity.as_str(),
        "creo:featdefs:sketch_entity#917:30"
    );
    assert_eq!(profiles[1].len(), 4);
    assert_eq!(
        profiles[1][0].entity.as_str(),
        "creo:featdefs:sketch_entity#917:10"
    );
}

#[test]
fn saved_profile_entity_identity_refuses_retained_limit() {
    let (sketch, geometries) = saved_profile_fixture();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
    let error = super::saved_profile_chains(&ctx, &sketch, &geometries)
        .expect_err("sketch entity identity exceeds retained limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.operation == "creo sketch entity identity"
            && resource.dimension == ResourceDimension::RetainedBytes)
    );
    crate::decode::with_test_decode_ctx(|ctx| {
        let checked = crate::decode::sketch_ids::sketch_entity_id_admitted(ctx, &sketch, 30)
            .expect("identity resources");
        assert_eq!(
            checked,
            crate::decode::sketch_ids::sketch_entity_id(&sketch, 30)
        );
    });
}

fn saved_profile_refuses_at_collection_boundary(operation: &'static str) {
    let mut last_refusal = None;
    for limit in 0..128 {
        let (sketch, geometries) = saved_profile_fixture();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
        match super::saved_profile_chains(&ctx, &sketch, &geometries) {
            Err(cadmpeg_core::CodecError::ResourceLimit(resource))
                if resource.dimension == ResourceDimension::CollectionItems
                    && resource.operation == operation =>
            {
                return
            }
            Err(cadmpeg_core::CodecError::ResourceLimit(resource)) => {
                last_refusal = Some((limit, resource.dimension, resource.operation));
            }
            Err(error) => panic!("unexpected saved-profile error at {limit}: {error:?}"),
            Ok(_) => panic!("saved profile succeeded before the named {operation} refusal"),
        }
    }
    panic!("the named {operation} boundary was not reached; last refusal: {last_refusal:?}");
}

macro_rules! saved_profile_collection_limit_test {
    ($name:ident, $operation:literal) => {
        #[test]
        fn $name() {
            saved_profile_refuses_at_collection_boundary($operation);
        }
    };
}

saved_profile_collection_limit_test!(
    saved_circular_profile_uses_refuse_collection_limit,
    "creo saved circular profile uses"
);
saved_profile_collection_limit_test!(
    saved_profile_rows_refuse_collection_limit,
    "creo saved profile rows"
);
saved_profile_collection_limit_test!(
    saved_profile_endpoint_rows_refuse_collection_limit,
    "creo saved profile endpoint rows"
);
saved_profile_collection_limit_test!(
    saved_profile_remaining_nodes_refuse_collection_limit,
    "creo saved profile remaining nodes"
);
saved_profile_collection_limit_test!(
    saved_profile_visited_nodes_refuse_collection_limit,
    "creo saved profile visited nodes"
);
saved_profile_collection_limit_test!(
    saved_profile_uses_refuse_collection_limit,
    "creo saved profile uses"
);
#[test]
fn numerical_ranges_section_arc_radius_agreement_has_no_length_floor() {
    let segment = FeatureSegment {
        kind: FeatureSegmentKind::Arc([1, 2]),
        directions: [None; 3],
        center_id: Some(3),
        arc_orientation: Some(0),
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id: 4,
        body: Vec::new(),
        offset: 9,
    };
    for radius in [1e-10, 1.0, 1e100] {
        for (factor, accepted) in [(1.0, true), (5.0, false)] {
            let points =
                BTreeMap::from([(1, [radius, 0.]), (2, [0., factor * radius]), (3, [0., 0.])]);
            assert_eq!(section_arc_geometry(&points, &segment).is_some(), accepted);
        }
    }
}

#[test]
fn numerical_ranges_saved_section_arc_rejects_different_tiny_radii() {
    use crate::feature::definitions::{
        DefinitionIdentity, FeatureDefinition, FeatureOrderRow, FeatureOrderTable, FeatureSavedArc,
        FeatureSavedEntity, FeatureSavedSection,
    };
    let segment = FeatureSegment {
        kind: FeatureSegmentKind::Arc([1, 2]),
        directions: [None; 3],
        center_id: Some(3),
        arc_orientation: Some(0),
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id: 4,
        body: Vec::new(),
        offset: 9,
    };
    for factor in [1.0, 5.0] {
        let radius = 1e-10;
        let definition = FeatureDefinition {
            identity: DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(5),
                owner_feature_id: Some(6),
            },
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: Some(FeatureOrderTable {
                declared_count: 1,
                has_prototype: false,
                entity_ref: None,
                rows: vec![FeatureOrderRow {
                    external_id: 4,
                    internal_id: 30,
                    bitmask: 0,
                    offset: 10,
                }],
                offset: 8,
            }),
            section_3d: None,
            dimensions: None,
            relations: None,
            saved_section: Some(FeatureSavedSection {
                entities: vec![FeatureSavedEntity::Arc(FeatureSavedArc {
                    entity_id: 30,
                    center: [Some(0.), Some(0.), Some(0.)],
                    radius: None,
                    endpoints: [
                        [Some(radius), Some(0.), Some(0.)],
                        [Some(0.), Some(radius * factor), Some(0.)],
                    ],
                    parameters: [None; 2],
                    body: Vec::new(),
                    offset: 20,
                })],
                offset: 18,
            }),
            offset: 0,
        };
        assert_eq!(
            saved_section_arc_carrier(&definition, &segment).is_some(),
            factor == 1.0
        );
    }
}

#[test]
fn numerical_ranges_saved_arc_entity_checks_endpoint_radii() {
    use crate::feature::definitions::{FeatureSavedArc, FeatureSavedEntity};
    const RADIUS: f64 = 1e-10;
    for (factor, accepted) in [(1.0, true), (5.0, false)] {
        let arc = FeatureSavedEntity::Arc(FeatureSavedArc {
            entity_id: 30,
            center: [Some(0.); 3],
            radius: Some(RADIUS),
            endpoints: [
                [Some(RADIUS), Some(0.), Some(0.)],
                [Some(0.), Some(RADIUS * factor), Some(0.)],
            ],
            parameters: [None; 2],
            body: Vec::new(),
            offset: 0,
        });
        assert_eq!(
            super::saved_section_entity_geometry(&arc).is_some(),
            accepted
        );
    }
}

fn saved_arc_carrier_definition(
    center: [Option<f64>; 3],
    radius: Option<f64>,
) -> (
    crate::feature::definitions::FeatureDefinition,
    FeatureSegment,
) {
    use crate::feature::definitions::{
        DefinitionIdentity, FeatureDefinition, FeatureOrderRow, FeatureOrderTable, FeatureSavedArc,
        FeatureSavedEntity, FeatureSavedSection,
    };
    let segment = FeatureSegment {
        kind: FeatureSegmentKind::Arc([1, 2]),
        directions: [None; 3],
        center_id: Some(3),
        arc_orientation: Some(0),
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id: 4,
        body: Vec::new(),
        offset: 9,
    };
    let definition = FeatureDefinition {
        identity: DefinitionIdentity::Parsed {
            schema_id: std::num::NonZeroU32::new(5),
            owner_feature_id: Some(6),
        },
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: None,
        segments: None,
        trim_entities: None,
        trim_vertices: None,
        order_table: Some(FeatureOrderTable {
            declared_count: 1,
            has_prototype: false,
            entity_ref: None,
            rows: vec![FeatureOrderRow {
                external_id: 4,
                internal_id: 30,
                bitmask: 0,
                offset: 10,
            }],
            offset: 8,
        }),
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: Some(FeatureSavedSection {
            entities: vec![FeatureSavedEntity::Arc(FeatureSavedArc {
                entity_id: 30,
                center,
                radius,
                endpoints: [
                    [Some(1.0), Some(0.0), Some(0.0)],
                    [Some(0.0), Some(1.0), Some(0.0)],
                ],
                parameters: [None; 2],
                body: Vec::new(),
                offset: 20,
            })],
            offset: 18,
        }),
        offset: 0,
    };
    (definition, segment)
}

#[test]
fn saved_arc_nonfinite_stored_radius_is_not_a_carrier() {
    let (definition, segment) = saved_arc_carrier_definition([Some(0.0); 3], Some(f64::INFINITY));
    assert!(saved_section_arc_carrier(&definition, &segment).is_none());
}

#[test]
fn saved_arc_nonfinite_stored_center_is_not_a_carrier() {
    let (definition, segment) =
        saved_arc_carrier_definition([Some(f64::NAN), Some(0.0), Some(0.0)], Some(2.0));
    assert!(saved_section_arc_carrier(&definition, &segment).is_none());
}

#[test]
fn saved_arc_overflowing_endpoint_radius_is_not_geometry() {
    let (mut definition, segment) = saved_arc_carrier_definition([Some(0.0); 3], Some(2.0));
    let Some(crate::feature::definitions::FeatureSavedEntity::Arc(arc)) = definition
        .saved_section
        .as_mut()
        .and_then(|section| section.entities.first_mut())
    else {
        panic!("saved arc fixture");
    };
    arc.endpoints[0] = [Some(f64::MAX), Some(f64::MAX), Some(0.0)];
    arc.endpoints[1] = [Some(0.0), Some(2.0), Some(0.0)];
    assert!(saved_section_arc(&definition, &segment).is_none());
}

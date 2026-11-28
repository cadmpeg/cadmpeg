// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{SketchGeometry, SketchGeometryDefinition, SketchId, SketchPlacement};

#[test]
fn an_unlisted_corner_arc_closes_the_selected_region() {
    let id = SketchId::mint("synthetic:test:sketch#rounded").unwrap();
    let ends = [
        ((0., 0.), (2., 0.)),
        ((2., 0.), (2., 1.)),
        ((1., 2.), (0., 2.)),
        ((0., 2.), (0., 0.)),
    ];
    let mut entities = ends
        .iter()
        .zip(1u64..)
        .map(|((a, b), primary)| {
            SketchEntity::new(
                crate::ids::neutral_sketch_curve_id(&id, primary, 0),
                id.clone(),
                SketchGeometry::try_from(SketchGeometryDefinition::Line {
                    start: Point2::new(a.0, a.1),
                    end: Point2::new(b.0, b.1),
                })
                .unwrap(),
            )
        })
        .collect::<Vec<_>>();
    let arc = crate::ids::neutral_sketch_curve_id(&id, 5, 0);
    entities.push(SketchEntity::new(
        arc.clone(),
        id.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Arc {
            center: Point2::new(1., 1.),
            radius: cadmpeg_ir::scalar::Length::new(1.).unwrap(),
            start_angle: cadmpeg_ir::scalar::Angle::new(0.).unwrap(),
            end_angle: cadmpeg_ir::scalar::Angle::new(std::f64::consts::FRAC_PI_2).unwrap(),
        })
        .unwrap(),
    ));
    let sketch = Sketch {
        id,
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::Unresolved {},
        profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
        native_ref: None,
    };
    let curves = (1u32..=5)
        .map(|primary| SketchCurveIdentity {
            id: format!("stream:curve-{primary}"),
            record_index: primary,
            owner_reference: Some(42),
            geometry_group: None,
            class_tag: "450".to_owned().try_into().unwrap(),
            byte_offset: 0,
            geometry_offset: 0,
            entity_genesis: None,
            primary_id: u64::from(primary).try_into().unwrap(),
            secondary_id: 0,
            geometry: None,
            planar_z_noise: None,
            tail_marker: None,
        })
        .collect::<Vec<_>>();
    let members = (1..=4).map(|primary| cadmpeg_core::json!({"kind_offset":0,"curve_primary_id":primary,"curve_primary_id_offset":0,"incidence_words_offset":0,"incidence_words":[0,0,0,0,1,1,0,0],"kind":3})).collect::<Vec<_>>();
    let profile: DesignSketchProfileOperand = cadmpeg_core::serde_erase::json::from_value(cadmpeg_core::json!({
        "scope_reference_ordinal":0,"record_index":1,"byte_offset":0,"class_tag":"411","asset_id":"00000000-0000-0000-0000-000000000001","asset_id_offset":1,"entity_id":"1_42","entity_suffix":42,"entity_reference_offset":2,"paired_class_tag":"262","paired_byte_offset":3,
        "region_selection":{"record_index":4,"byte_offset":0,"class_tag":"382","region_count_offset":0,"regions":[{"member_count_offset":0,"members":members}],"companion_class_tag":"291","companion_byte_offset":0}
    })).unwrap();
    crate::test_support::with_decode_context(|ctx| {
        let work = crate::design::geometry::ProfileGeometryWork::new();
        let histories = std::collections::HashMap::new();
        let resolution = crate::design::profile_select::ExtrudeProfileResolution {
            entities: &entities,
            spatial_sketches: &[],
            spatial_entities: &[],
            histories: &[],
            scope_histories: &histories,
            linear_tolerance: 1.0e-7,
            angular_tolerance: 1.0e-9,
            profile_work: &work,
            ctx,
        }
        .scoped(&[]);
        assert!(
            resolve("stream", &profile, &sketch, &curves, &entities, resolution, true)?.is_none()
        );
        let selected =
            resolve_arc_bridges("stream", &profile, &sketch, &curves, &entities, resolution)?
                .unwrap();
        let PlanarProfileRef::SketchRegions { regions, .. } = selected else {
            panic!("explicit region")
        };
        let [cadmpeg_ir::features::profiles::SketchProfileRegion::Trimmed {
            outer_boundary,
            hole_boundaries,
        }] = regions.as_slice()
        else {
            panic!("one trimmed region")
        };
        assert!(hole_boundaries.is_empty());
        assert!(outer_boundary.iter().any(|use_| use_.entity == arc));
        let mut ambiguous = entities.clone();
        ambiguous.push(SketchEntity::new(
            crate::ids::neutral_sketch_curve_id(&sketch.id, 6, 0),
            sketch.id.clone(),
            entities.last().unwrap().geometry.clone(),
        ));
        assert!(
            resolve_arc_bridges("stream", &profile, &sketch, &curves, &ambiguous, resolution,)?
                .is_none()
        );
        Ok::<_, CodecError>(())
    })
    .unwrap();
}

fn line(id: &SketchId, primary: u64, a: (f64, f64), b: (f64, f64)) -> SketchEntity {
    SketchEntity::new(
        crate::ids::neutral_sketch_curve_id(id, primary, 0),
        id.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(a.0, a.1),
            end: Point2::new(b.0, b.1),
        })
        .unwrap(),
    )
}

fn arc(
    id: &SketchId,
    primary: u64,
    center: (f64, f64),
    radius: f64,
    angles: (f64, f64),
) -> SketchEntity {
    SketchEntity::new(
        crate::ids::neutral_sketch_curve_id(id, primary, 0),
        id.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Arc {
            center: Point2::new(center.0, center.1),
            radius: cadmpeg_ir::scalar::Length::new(radius).unwrap(),
            start_angle: cadmpeg_ir::scalar::Angle::new(angles.0).unwrap(),
            end_angle: cadmpeg_ir::scalar::Angle::new(angles.1).unwrap(),
        })
        .unwrap(),
    )
}

fn sketch_of(id: SketchId) -> Sketch {
    Sketch {
        id,
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::Unresolved {},
        profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
        native_ref: None,
    }
}

fn curve_identity(primary: u32, tail_marker: Option<u8>) -> SketchCurveIdentity {
    SketchCurveIdentity {
        id: format!("stream:curve-{primary}"),
        record_index: primary,
        owner_reference: Some(42),
        geometry_group: None,
        class_tag: "450".to_owned().try_into().unwrap(),
        byte_offset: 0,
        geometry_offset: 0,
        entity_genesis: None,
        primary_id: u64::from(primary).try_into().unwrap(),
        secondary_id: 0,
        geometry: None,
        planar_z_noise: None,
        tail_marker,
    }
}

/// A selection that lists curves `1..=listed` as one counted ring.
fn operand(listed: u32) -> DesignSketchProfileOperand {
    let members = (1..=listed).map(|primary| cadmpeg_core::json!({"kind_offset":0,"curve_primary_id":primary,"curve_primary_id_offset":0,"incidence_words_offset":0,"incidence_words":[0,0,0,0,1,1,0,0],"kind":3})).collect::<Vec<_>>();
    cadmpeg_core::serde_erase::json::from_value(cadmpeg_core::json!({
        "scope_reference_ordinal":0,"record_index":1,"byte_offset":0,"class_tag":"411","asset_id":"00000000-0000-0000-0000-000000000001","asset_id_offset":1,"entity_id":"1_42","entity_suffix":42,"entity_reference_offset":2,"paired_class_tag":"262","paired_byte_offset":3,
        "region_selection":{"record_index":4,"byte_offset":0,"class_tag":"382","region_count_offset":0,"regions":[{"member_count_offset":0,"members":members}],"companion_class_tag":"291","companion_byte_offset":0}
    })).unwrap()
}

/// The outcomes of the normal path (assumptions refused, then allowed) and of
/// the bridge path.
fn outcomes(
    listed: u32,
    sketch: &Sketch,
    entities: &[SketchEntity],
    curves: &[SketchCurveIdentity],
) -> [bool; 3] {
    let profile = operand(listed);
    crate::test_support::with_decode_context(|ctx| {
        let work = crate::design::geometry::ProfileGeometryWork::new();
        let histories = std::collections::HashMap::new();
        let resolution = crate::design::profile_select::ExtrudeProfileResolution {
            entities,
            spatial_sketches: &[],
            spatial_entities: &[],
            histories: &[],
            scope_histories: &histories,
            linear_tolerance: 1.0e-7,
            angular_tolerance: 1.0e-9,
            profile_work: &work,
            ctx,
        }
        .scoped(&[]);
        let strict = resolve(
            "stream", &profile, sketch, curves, entities, resolution, false,
        )?;
        let lenient = resolve(
            "stream", &profile, sketch, curves, entities, resolution, true,
        )?;
        let bridged =
            resolve_arc_bridges("stream", &profile, sketch, curves, entities, resolution)?;
        Ok::<_, CodecError>([strict.is_some(), lenient.is_some(), bridged.is_some()])
    })
    .unwrap()
}

/// Three lines of a rounded rectangle; curve 4 is the unlisted corner arc and
/// `extra` entities follow it.
fn rounded_with(extra: Vec<SketchEntity>, marker: Option<u8>) -> [bool; 3] {
    let id = SketchId::mint("synthetic:test:sketch#rounded").unwrap();
    let mut entities = vec![
        line(&id, 1, (0., 0.), (2., 0.)),
        line(&id, 2, (2., 0.), (2., 1.)),
        line(&id, 3, (1., 2.), (0., 2.)),
        line(&id, 4, (0., 2.), (0., 0.)),
    ];
    entities.push(arc(&id, 5, (1., 1.), 1., (0., std::f64::consts::FRAC_PI_2)));
    entities.extend(extra);
    if marker.is_some() {
        entities[4].native_ref = Some("stream:curve-5".to_owned());
    }
    let curves = (1u32..=u32::try_from(entities.len()).unwrap())
        .map(|primary| curve_identity(primary, (primary == 5).then_some(marker).flatten()))
        .collect::<Vec<_>>();
    outcomes(4, &sketch_of(id), &entities, &curves)
}

fn rounded_id() -> SketchId {
    SketchId::mint("synthetic:test:sketch#rounded").unwrap()
}

#[test]
fn an_unlisted_arc_that_a_counted_ring_never_names_is_not_a_bridge() {
    assert_eq!(rounded_with(Vec::new(), None), [false, false, true]);
    assert_eq!(rounded_with(Vec::new(), Some(1)), [false, false, false]);
}

#[test]
fn a_construction_arc_or_a_full_circle_is_not_a_bridge() {
    let id = rounded_id();
    let mut construction = arc(&id, 5, (1., 1.), 1., (0., std::f64::consts::FRAC_PI_2));
    construction.construction = true;
    let circle = arc(&id, 5, (1., 1.), 1., (0., std::f64::consts::TAU));
    for replacement in [construction, circle] {
        let mut entities = vec![
            line(&id, 1, (0., 0.), (2., 0.)),
            line(&id, 2, (2., 0.), (2., 1.)),
            line(&id, 3, (1., 2.), (0., 2.)),
            line(&id, 4, (0., 2.), (0., 0.)),
        ];
        entities.push(replacement);
        let curves = (1u32..=5)
            .map(|primary| curve_identity(primary, None))
            .collect::<Vec<_>>();
        assert_eq!(
            outcomes(4, &sketch_of(id.clone()), &entities, &curves),
            [false, false, false]
        );
    }
}

#[test]
fn an_arc_and_a_line_that_both_fit_give_no_bridge() {
    let id = rounded_id();
    let chord = line(&id, 6, (2., 1.), (1., 2.));
    assert_eq!(rounded_with(vec![chord], None), [false, false, false]);
}

#[test]
fn a_unique_joining_line_is_not_a_bridge() {
    let id = rounded_id();
    let mut entities = vec![
        line(&id, 1, (0., 0.), (2., 0.)),
        line(&id, 2, (2., 0.), (2., 1.)),
        line(&id, 3, (1., 2.), (0., 2.)),
        line(&id, 4, (0., 2.), (0., 0.)),
    ];
    entities.push(line(&id, 5, (2., 1.), (1., 2.)));
    let curves = (1u32..=5)
        .map(|primary| curve_identity(primary, None))
        .collect::<Vec<_>>();
    assert_eq!(
        outcomes(4, &sketch_of(id), &entities, &curves),
        [false, false, false]
    );
}

#[test]
fn a_region_refused_only_by_an_assumption_gate_is_not_bridged() {
    // The listed rectangle closes a region only when the island absence of an
    // ellipse that crosses its side is assumed. An arc outside the rectangle
    // joins two of its corners and leaves it closed.
    let id = rounded_id();
    let mut entities = vec![
        line(&id, 1, (0., 0.), (2., 0.)),
        line(&id, 2, (2., 0.), (2., 2.)),
        line(&id, 3, (2., 2.), (0., 2.)),
        line(&id, 4, (0., 2.), (0., 0.)),
    ];
    entities.push(arc(
        &id,
        5,
        (2., 2.),
        2.,
        (-std::f64::consts::FRAC_PI_2, std::f64::consts::PI),
    ));
    entities.push(SketchEntity::new(
        crate::ids::neutral_sketch_curve_id(&id, 6, 0),
        id.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Ellipse {
            center: Point2::new(2.5, 1.),
            major_angle: cadmpeg_ir::scalar::Angle::new(0.).unwrap(),
            radii: cadmpeg_ir::sketches::EllipseRadii {
                major_radius: cadmpeg_ir::scalar::Length::new(1.).unwrap(),
                minor_radius: cadmpeg_ir::scalar::Length::new(0.5).unwrap(),
            },
            bounds: None,
        })
        .unwrap(),
    ));
    let curves = (1u32..=6)
        .map(|primary| curve_identity(primary, None))
        .collect::<Vec<_>>();
    assert_eq!(
        outcomes(4, &sketch_of(id), &entities, &curves),
        [false, true, false]
    );
}

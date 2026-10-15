// SPDX-License-Identifier: Apache-2.0

use crate::sketches::{SketchConstraintDefinitionInput, SketchLocus};

#[test]
fn sketch_constraint_loci_borrow_elements_in_order() {
    let definition = SketchConstraintDefinitionInput::Group {
        elements: vec![
            SketchLocus::Start("test:model:entity#loci".try_into().unwrap()),
            SketchLocus::End("test:model:entity#end".try_into().unwrap()),
            SketchLocus::Center("test:model:entity#center".try_into().unwrap()),
        ],
    };
    let SketchConstraintDefinitionInput::Group { elements } = &definition else {
        panic!("group fixture");
    };
    let loci = super::super::constraint_loci(&definition);
    assert_eq!(loci.len(), elements.len());
    for (actual, expected) in loci.iter().zip(elements) {
        assert!(std::ptr::eq(*actual, expected));
    }
}

#[test]
fn sketch_nurbs_endpoints_read_rational_poles_without_copying_all_rows() {
    let curve = crate::geometry::pcurve::PcurveNurbs::from_lanes(
        1,
        vec![0.0, 0.0, 0.5, 1.0, 1.0],
        vec![
            crate::math::Point2::new(0.0, 0.0),
            crate::math::Point2::new(2.0, 8.0),
            crate::math::Point2::new(3.0, 4.0),
        ],
        Some(vec![1.0, 2.0, 1.0]),
        false,
    )
    .unwrap();
    let geometry = crate::sketches::SketchGeometry::nurbs(curve);
    let endpoints = (
        crate::math::Point2::new(0.0, 0.0),
        crate::math::Point2::new(3.0, 4.0),
    );
    assert_eq!(
        super::super::oriented_endpoints(&geometry, false),
        Some(endpoints)
    );
    assert_eq!(
        super::super::oriented_endpoints(&geometry, true),
        Some((endpoints.1, endpoints.0))
    );
}

fn index_fixture(kind: usize) -> crate::CadIr {
    use crate::sketches::{
        SketchEntity, SketchGeometry, SketchGeometryDefinition, SpatialSketch, SpatialSketchEntity,
        SpatialSketchGeometry, SpatialSketchGeometryDefinition,
    };
    let mut ir = crate::CadIr::empty();
    for key in ["first", "second"] {
        match kind {
            0 => ir.model.sketch_entities.push(SketchEntity::new(
                format!("test:model:entity#{key}").try_into().unwrap(),
                "test:model:sketch#owner".try_into().unwrap(),
                SketchGeometry::try_from(SketchGeometryDefinition::Point {
                    position: crate::math::Point2::new(1.0, 2.0),
                })
                .unwrap(),
            )),
            1 => ir.model.spatial_sketches.push(SpatialSketch {
                id: format!("test:model:sketch#{key}").try_into().unwrap(),
                name: None,
                configuration: None,
                visible: None,
                profiles: vec![],
                native_ref: None,
            }),
            2 => ir
                .model
                .spatial_sketch_entities
                .push(SpatialSketchEntity::new(
                    format!("test:model:entity#{key}").try_into().unwrap(),
                    "test:model:sketch#owner".try_into().unwrap(),
                    SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Point {
                        position: crate::math::Point3::new(1.0, 2.0, 3.0),
                    })
                    .unwrap(),
                )),
            3 => ir.model.parameters.push(crate::features::DesignParameter {
                id: format!("test:model:parameter#{key}").try_into().unwrap(),
                owner: None,
                ordinal: 0,
                name: key.into(),
                expression: "1 mm".into(),
                display: None,
                value: Some(crate::features::ParameterValue::Length(
                    crate::scalar::Length::new(1.0).unwrap(),
                )),
                dependencies: crate::features::DistinctMembers::default(),
                properties: std::collections::BTreeMap::default(),
                pmi: None,
                native_ref: None,
            }),
            _ => panic!("index fixture kind"),
        }
    }
    ir
}

#[test]
fn projected_sketch_copy_keeps_missing_member_semantics_and_finding_order() {
    let mut ir = index_fixture(0);
    ir.model.sketch_entities[1].geometry = crate::sketches::SketchGeometry::try_from(
        crate::sketches::SketchGeometryDefinition::Point {
            position: crate::math::Point2::new(3.0, 4.0),
        },
    )
    .unwrap();
    for (key, result) in [
        ("mismatch", ir.model.sketch_entities[1].id().clone()),
        ("missing", "test:model:entity#missing".try_into().unwrap()),
    ] {
        ir.model
            .sketch_constraints
            .push(crate::sketches::SketchConstraint {
                id: format!("test:model:constraint#{key}").try_into().unwrap(),
                sketch: ir.model.sketch_entities[0].sketch.clone(),
                definition: SketchConstraintDefinitionInput::ProjectedCopy {
                    source: ir.model.sketch_entities[0].id().clone(),
                    result,
                }
                .try_into()
                .unwrap(),
                name: None,
                driving: None,
                active: None,
                virtual_space: None,
                visible: None,
                orientation: None,
                label_distance: None,
                label_position: None,
                metadata: None,
                native_ref: None,
            });
    }
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut findings = vec![];
    super::super::check_sketches(&ctx, &ir, &mut findings).unwrap();
    assert_eq!(findings.len(), 1);
    assert_eq!(
        findings[0].check,
        crate::report::check::Check::GeometricConsistency
    );
    assert_eq!(findings[0].severity, crate::report::Severity::Error);
    assert_eq!(
        findings[0].entity.as_deref(),
        Some("test:model:constraint#mismatch")
    );
    assert_eq!(
        findings[0].message,
        "projected-copy entities do not have identical geometry"
    );
    ctx.finish_session().unwrap();
}

#[test]
fn sketch_index_helpers_keep_last_duplicate_values() {
    use crate::features::{ParameterId, ParameterValue};
    use crate::index::identities::BorrowedIdentities;
    use crate::sketches::{SketchGeometry, SketchGeometryDefinition};
    let id: crate::sketches::SketchEntityId = "test:model:entity#same".try_into().unwrap();
    let point = |u, v| {
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: crate::math::Point2::new(u, v),
        })
        .unwrap()
    };
    let first = point(1.0, 2.0);
    let last = point(3.0, 4.0);
    let geometry = BorrowedIdentities::build(|add| {
        add(id.as_str(), &first)?;
        add(id.as_str(), &last)
    })
    .unwrap();
    let locus = SketchLocus::Entity(id.clone());
    assert_eq!(
        super::super::sketch_locus_point(&locus, &geometry).unwrap(),
        Some(crate::math::Point2::new(3.0, 4.0))
    );
    let restriction = SketchConstraintDefinitionInput::Midpoint {
        point: locus.clone(),
        entity: id.clone(),
    };
    assert_eq!(
        super::super::constraint_entity_kind_refusal(&restriction, &geometry),
        Some("sketch midpoint constraint references an entity that is not a bounded curve")
    );
    let parameter: ParameterId = "test:model:parameter#same".try_into().unwrap();
    let first = Some(ParameterValue::Length(
        crate::scalar::Length::new(1.0).unwrap(),
    ));
    let last = Some(ParameterValue::Length(
        crate::scalar::Length::new(2.0).unwrap(),
    ));
    let parameters = BorrowedIdentities::build(|add| {
        add(parameter.as_str(), &first)?;
        add(parameter.as_str(), &last)
    })
    .unwrap();
    assert!(super::super::spatial_length_parameter_matches(
        Some(2.0),
        &parameter,
        &parameters
    ));
    assert!(!super::super::spatial_length_parameter_matches(
        Some(1.0),
        &parameter,
        &parameters
    ));
}

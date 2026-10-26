// SPDX-License-Identifier: Apache-2.0
use super::{assert_written_arenas_agree, neutral_view, translate_model, written_arenas};
use cadmpeg_ir::codec::write::{target::TargetRequest, CadirEncoder, EncodeInput, Encoder};
use cadmpeg_ir::examples::unit_cube;
use cadmpeg_ir::{ArenaClass, CadIr};

#[test]
fn translation_moves_carriers_and_keeps_native_records_and_directions() {
    let mut ir = unit_cube().expect("cube fixture");
    let before = ir.clone();
    translate_model(&mut ir, [13.0, -7.0, 5.0]).expect("finite translation");
    assert_eq!(ir.native, before.native);
    for (before, after) in before.model.points.iter().zip(&ir.model.points) {
        let before = before.position().get();
        let after = after.position().get();
        assert_eq!(
            [after.x, after.y, after.z],
            [before.x + 13.0, before.y - 7.0, before.z + 5.0]
        );
    }
    let before = serde_json::to_value(&before.model.curves).expect("curve projection");
    let after = serde_json::to_value(&ir.model.curves).expect("curve projection");
    for (before, after) in before
        .as_array()
        .expect("curves")
        .iter()
        .zip(after.as_array().expect("curves"))
    {
        assert_eq!(
            before["geometry"]["direction"],
            after["geometry"]["direction"]
        );
    }
    let validation =
        cadmpeg_ir::validate::validate_neutral(&ir, Vec::new()).expect("validate translated cube");
    assert!(
        !validation
            .findings
            .iter()
            .any(|finding| finding.severity >= cadmpeg_ir::report::Severity::Error),
        "{:?}",
        validation.findings
    );
}

#[test]
fn translation_moves_polynomial_and_rational_surface_poles_once() {
    use cadmpeg_ir::geometry::nurbs::{
        NurbsPoleGrid, NurbsSurface, NurbsSurfaceAxis, WeightedPole3,
    };
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry};
    use cadmpeg_ir::math::Point3;
    let rows = vec![
        vec![Point3::new(1.0, 2.0, 3.0), Point3::new(4.0, 5.0, 6.0)],
        vec![Point3::new(7.0, 8.0, 9.0), Point3::new(10.0, 11.0, 12.0)],
    ];
    for rational in [false, true] {
        let poles = if rational {
            NurbsPoleGrid::Rational {
                rows: rows
                    .iter()
                    .map(|row| {
                        row.iter()
                            .enumerate()
                            .map(|(index, point)| WeightedPole3 {
                                point: *point,
                                weight: cadmpeg_ir::scalar::NonZeroReal::new(if index == 0 {
                                    2.0
                                } else {
                                    4.0
                                })
                                .expect("valid authored test fixture"),
                            })
                            .collect()
                    })
                    .collect(),
            }
        } else {
            NurbsPoleGrid::Polynomial { rows: rows.clone() }
        };
        let axis = || NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false);
        let surface =
            NurbsSurface::new(axis(), axis(), poles, false).expect("valid authored test fixture");
        let weights = surface.weights();
        let mut ir = CadIr::empty();
        ir.model.surfaces.push(Surface {
            native_ref: None,
            id: cadmpeg_ir::ids::SurfaceId::mint("test:model:surface#translate")
                .expect("valid authored test fixture"),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface)),
            source_object: None,
        });
        translate_model(&mut ir, [13.0, -7.0, 5.0]).expect("valid authored test fixture");
        let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface)) =
            &ir.model.surfaces[0].geometry
        else {
            panic!("NURBS surface stays a NURBS surface");
        };
        assert_eq!(surface.weights(), weights);
        for (before, after) in rows.iter().flatten().zip(surface.poles()) {
            assert_eq!(
                after.get(),
                Point3::new(before.x + 13.0, before.y - 7.0, before.z + 5.0)
            );
        }
    }
}

#[test]
fn invalid_translation_leaves_document_unchanged() {
    let mut ir = unit_cube().expect("cube fixture");
    let before = ir.clone();
    assert!(translate_model(&mut ir, [f64::INFINITY, 0.0, 0.0]).is_err());
    assert_eq!(ir, before);
}

#[test]
fn neutral_projection_removes_declared_classes_and_provenance_layer() {
    let ir = unit_cube().expect("cube fixture");
    let view = neutral_view(&ir, &[ArenaClass::Model]);
    assert!(view.get("scope").is_none());
    assert!(view.get("native").is_none());
    assert!(view["model"].get("bodies").is_none());
    assert!(view["model"].get("parameters").is_some());
}

#[test]
fn written_comparison_observes_only_claimed_arenas() {
    let ir = CadIr::empty();
    let plan = CadirEncoder
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("CADIR plan");
    assert_eq!(
        written_arenas(plan.report()).len(),
        cadmpeg_ir::ArenaName::ALL.len()
    );
    assert_written_arenas_agree(plan.report(), &ir, &ir);
}

#[test]
fn written_comparison_normalizes_only_arena_storage_order() {
    let ir = unit_cube().expect("cube fixture");
    let plan = CadirEncoder
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("CADIR plan");
    let mut reordered = ir.clone();
    reordered.model.points.reverse();
    reordered.model.curves.reverse();
    reordered.model.faces.reverse();
    assert_written_arenas_agree(plan.report(), &ir, &reordered);
    reordered.model.points.pop();
    assert!(
        std::panic::catch_unwind(|| {
            assert_written_arenas_agree(plan.report(), &ir, &reordered);
        })
        .is_err(),
        "a missing written entity must fail the content comparison"
    );
}

#[test]
fn written_comparison_accepts_only_consistent_global_identity_renaming() {
    let mut ir = unit_cube().expect("cube fixture");
    ir.model.points[0].source_object = Some(
        serde_json::from_value(serde_json::json!({
            "format":"rhino", "object_id":"11111111-1111-1111-1111-111111111111",
            "instance_path":["instance-old"]
        }))
        .expect("typed source association"),
    );
    ir.model.bodies[0].name = Some("11111111-1111-1111-1111-111111111111".into());
    let plan = CadirEncoder
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("CADIR plan");
    let (_, marked, _) = super::identity::mark::model(&ir);
    let mut renamed = ir.clone();
    renamed.model = serde_json::from_value(marked).expect("renamed model remains valid");
    assert_written_arenas_agree(plan.report(), &ir, &renamed);
    renamed.model.bodies[0].name = Some("22222222-2222-2222-2222-222222222222".into());
    assert!(
        std::panic::catch_unwind(|| assert_written_arenas_agree(plan.report(), &ir, &renamed))
            .is_err(),
        "an identity-looking display name remains ordinary content"
    );
}

#[test]
fn written_comparison_does_not_treat_excluded_design_as_known_empty() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_ir::document::ContentScope;
    use cadmpeg_ir::features::DesignParameter;
    // A test codec's typed native parameter is queryable document content,
    // even while its neutral design projection has not been run.
    let parameter: DesignParameter = serde_json::from_value(serde_json::json!({
        "id":"test:native:parameter#dimension", "name":"Length",
        "expression":{"tokens":[{"kind":"text","value":"37 mm"}]}, "driving":true,
        "value":{"kind":"length","value":37.0}
    }))
    .expect("typed native parameter");
    let mut before = CadIr::empty();
    before
        .set_scope(ContentScope::geometry())
        .expect("empty design may be excluded");
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default())
        .expect("valid authored test fixture");
    before
        .native
        .namespace_mut("test")
        .set_arena(&ctx, "parameters", std::slice::from_ref(&parameter))
        .expect("retain typed native parameter");
    let retained = before
        .native
        .namespace("test")
        .expect("valid authored test fixture")
        .arena_as::<DesignParameter>("parameters")
        .expect("typed native parameter query");
    assert_eq!(retained, vec![parameter.clone()]);
    let mut after = before.clone();
    after
        .set_scope(ContentScope::full())
        .expect("declare projected design");
    let mut projected = parameter;
    projected.native_ref = Some(projected.id.to_string());
    projected.id = "test:model:parameter#dimension"
        .try_into()
        .expect("valid authored test fixture");
    after.model.parameters.push(projected);
    let plan = CadirEncoder
        .plan(EncodeInput::new(&before, None), TargetRequest::Inherit)
        .expect("valid authored test fixture");
    assert_written_arenas_agree(plan.report(), &before, &after);

    before
        .set_scope(ContentScope::full())
        .expect("known complete empty design");
    let plan = CadirEncoder
        .plan(EncodeInput::new(&before, None), TargetRequest::Inherit)
        .expect("valid authored test fixture");
    assert!(
        std::panic::catch_unwind(|| {
            assert_written_arenas_agree(plan.report(), &before, &after);
        })
        .is_err(),
        "held empty design must not acquire undeclared neutral records"
    );
}

#[test]
fn geometry_projection_resolves_bound_pmi_without_losing_model_semantics() {
    use cadmpeg_ir::features::{DesignParameter, Dimension, ParameterId, ParameterValue};
    use cadmpeg_ir::ids::PmiId;
    use cadmpeg_ir::pmi::{
        DimensionKind, PmiAnnotation, PmiDefinition, PmiDimension, PmiQuantity, PmiValue,
    };
    use cadmpeg_ir::scalar::Length;

    let mut full = unit_cube().expect("cube fixture");
    let parameter_id = ParameterId::mint("synthetic:test:parameter#pmi-nominal")
        .expect("valid authored test fixture");
    let cache = ParameterValue::Length(Length::new(37.0).expect("valid authored test fixture"));
    full.model.parameters.push(DesignParameter {
        id: parameter_id.clone(),
        owner: None,
        ordinal: 0,
        name: "nominal".into(),
        expression: cache.literal_expression(),
        driving: true,
        display: None,
        value: Some(cache),
        properties: std::collections::BTreeMap::default(),
        pmi: None,
        native_ref: None,
    });
    full.model.pmi.push(PmiAnnotation {
        native_ref: None,
        id: PmiId::mint("synthetic:test:pmi#bound-nominal").expect("valid authored test fixture"),
        name: Some("width".into()),
        visible: Some(true),
        targets: Vec::new(),
        definition: PmiDefinition::Dimension(
            PmiDimension::new(
                DimensionKind::Size,
                Some(Dimension::Parameter(parameter_id)),
                None,
            )
            .expect("valid authored test fixture"),
        ),
    });
    let mut geometry = full.clone();
    geometry.model.parameters.clear();
    let PmiDefinition::Dimension(dimension) = &mut geometry.model.pmi[0].definition else {
        panic!("dimensional PMI fixture")
    };
    dimension
        .set_nominal(Some(
            PmiValue::new(37.0, PmiQuantity::Length)
                .expect("valid authored test fixture")
                .into(),
        ))
        .expect("valid authored test fixture");
    geometry
        .set_scope(cadmpeg_ir::ContentScope::geometry())
        .expect("valid authored test fixture");
    let full_view = neutral_view(&full, &[ArenaClass::Design]);
    assert_eq!(
        full_view["model"]["pmi"]
            .as_array()
            .expect("valid authored test fixture")
            .len(),
        1
    );
    assert_eq!(full_view, neutral_view(&geometry, &[ArenaClass::Design]));
    let PmiDefinition::Dimension(dimension) = &mut geometry.model.pmi[0].definition else {
        panic!("dimensional PMI fixture")
    };
    dimension
        .set_nominal(Some(
            PmiValue::new(38.0, PmiQuantity::Length)
                .expect("valid authored test fixture")
                .into(),
        ))
        .expect("valid authored test fixture");
    assert_ne!(full_view, neutral_view(&geometry, &[ArenaClass::Design]));
    assert!(
        matches!(
            &full.model.pmi[0].definition,
            PmiDefinition::Dimension(dimension)
                if matches!(dimension.nominal(), Some(Dimension::Parameter(_)))
        ),
        "projection must leave the authored binding unchanged"
    );
}

#[test]
fn geometry_projection_removes_only_attributes_owned_by_design_entities() {
    use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
    use cadmpeg_ir::features::{FeatureId, ParameterId};
    use cadmpeg_ir::ids::AttributeId;
    use cadmpeg_ir::sketches::SketchId;

    let mut full = unit_cube().expect("cube fixture");
    full.source = Some(
        serde_json::from_value(serde_json::json!({
            "identity": {"classification": "unclassified", "format": "synthetic"},
            "attributes": {}
        }))
        .expect("source content metadata"),
    );
    let targets = [
        AttributeTarget::Feature(
            FeatureId::mint("synthetic:test:feature#design-attribute")
                .expect("valid authored test fixture"),
        ),
        AttributeTarget::Parameter(
            ParameterId::mint("synthetic:test:parameter#design-attribute")
                .expect("valid authored test fixture"),
        ),
        AttributeTarget::Sketch(
            SketchId::mint("synthetic:test:sketch#design-attribute")
                .expect("valid authored test fixture"),
        ),
        AttributeTarget::Document,
        AttributeTarget::Body(full.model.bodies[0].id.clone()),
    ];
    for (index, target) in targets.into_iter().enumerate() {
        full.model.attributes.push(SourceAttribute {
            native_ref: None,
            id: AttributeId::mint(format!("synthetic:test:attribute#scope-{index}"))
                .expect("valid authored test fixture"),
            target,
            name: "value".into(),
            values: vec![Some(AttributeValue::Integer(1))],
        });
    }
    let mut geometry = full.clone();
    geometry.model.attributes.drain(..3);
    let projected = neutral_view(&full, &[ArenaClass::Design]);
    assert_eq!(
        projected["model"]["attributes"]
            .as_array()
            .expect("valid authored test fixture")
            .len(),
        2
    );
    assert_eq!(projected, neutral_view(&geometry, &[ArenaClass::Design]));
    geometry.model.attributes[0].values = vec![Some(AttributeValue::Integer(2))];
    assert_ne!(projected, neutral_view(&geometry, &[ArenaClass::Design]));
    geometry.model.attributes[0].values = vec![Some(AttributeValue::Integer(1))];
    geometry.model.attributes[1].values = vec![Some(AttributeValue::Integer(2))];
    assert_ne!(projected, neutral_view(&geometry, &[ArenaClass::Design]));
    let mut metadata_edit = full.clone();
    metadata_edit
        .source
        .as_mut()
        .expect("valid authored test fixture")
        .attributes
        .insert(
            cadmpeg_core::text::NonBlankString::new("test.local_digest")
                .expect("valid authored test fixture"),
            "content".into(),
        );
    assert_ne!(
        projected,
        neutral_view(&metadata_edit, &[ArenaClass::Design])
    );
}

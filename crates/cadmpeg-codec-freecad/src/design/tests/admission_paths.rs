// SPDX-License-Identifier: Apache-2.0
//! Regression tests for design-history admission paths.

use std::collections::{BTreeMap, HashMap};

use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{DesignParameter, DistinctMembers, FeatureDefinition, FeatureId};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{
    SketchConstraintDefinitionInput, SketchEntity, SketchGeometry, SketchGeometryDefinition,
    SketchId,
};

const WORK_MARKER: &str = "test FreeCAD design admission work marker";

fn object(
    id: &str,
    type_name: &str,
    order: usize,
    dependencies: Vec<String>,
) -> crate::native::ObjectRecord {
    let name = id.rsplit_once('#').map_or(id, |(_, key)| key);
    crate::native::ObjectRecord {
        identity: crate::native::object_identity::ObjectIdentity::try_new(
            id.to_owned(),
            name.to_owned(),
        )
        .expect("object identity"),
        type_name: type_name.to_owned(),
        persistent_id: None,
        view_type: None,
        attributes: BTreeMap::new(),
        dependencies,
        dependency_allow_partial: None,
        order,
        data: None,
    }
}

fn property(
    owner: &str,
    name: &str,
    type_name: &str,
    xml: impl Into<String>,
) -> crate::native::PropertyRecord {
    crate::native::PropertyRecord {
        id: format!("{owner}:{name}"),
        owner: owner.to_owned(),
        name: name.to_owned(),
        type_name: type_name.to_owned(),
        family: crate::native::PropertyFamily::Unknown,
        status: None,
        body: crate::native::PropertyBody::Transient,
        order: 0,
        xml: crate::native::RetainedXml::from_text(xml.into(), 0).expect("valid property XML"),
    }
}

fn placement_property(owner: &str, attributes: &[(&str, &str)]) -> crate::native::PropertyRecord {
    crate::native::PropertyRecord {
        id: format!("{owner}:Placement"),
        owner: owner.to_owned(),
        name: "Placement".to_owned(),
        type_name: "App::PropertyPlacement".to_owned(),
        family: crate::native::PropertyFamily::Unknown,
        status: None,
        body: crate::native::PropertyBody::Persisted {
            values: vec![crate::native::ValueRecord {
                tag: "PropertyPlacement".to_owned(),
                order: 0,
                attributes: attributes
                    .iter()
                    .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                    .collect(),
                text: None,
                raw_xml: String::new(),
            }],
            links: Vec::new(),
            side_entries: Vec::new(),
            dynamic: None,
        },
        order: 0,
        xml: crate::native::RetainedXml::from_text("<Property/>".to_owned(), 0)
            .expect("valid property XML"),
    }
}

fn constraint_property(owner: &str, xml: &str) -> crate::native::PropertyRecord {
    property(
        owner,
        "Constraints",
        "Sketcher::PropertyConstraintList",
        xml,
    )
}

fn geometry_property(owner: &str, xml: &str) -> crate::native::PropertyRecord {
    property(owner, "Geometry", "Part::PropertyGeometryList", xml)
}

fn membership_property(owner: &str, members: &[&str]) -> crate::native::PropertyRecord {
    let links = members
        .iter()
        .map(|member| {
            crate::native::LinkTarget::optional_from_wire(crate::native::LinkTargetWire::<String> {
                document: None,
                document_attribute: None,
                object: Some((*member).to_owned()),
                subelements: Vec::new(),
            })
            .expect("valid member link")
        })
        .collect();
    crate::native::PropertyRecord {
        id: format!("{owner}:Group"),
        owner: owner.to_owned(),
        name: "Group".to_owned(),
        type_name: "App::PropertyLinkList".to_owned(),
        family: crate::native::PropertyFamily::Unknown,
        status: None,
        body: crate::native::PropertyBody::Persisted {
            values: Vec::new(),
            links,
            side_entries: Vec::new(),
            dynamic: None,
        },
        order: 0,
        xml: crate::native::RetainedXml::from_text("<Property/>".to_owned(), 0)
            .expect("valid property XML"),
    }
}

fn work_used_before_marker(action: impl Fn(&DecodeContext<'_>) -> Result<(), CodecError>) -> u64 {
    let error =
        crate::test_support::refusal_at(ResourceDimension::WorkUnits, &[], WORK_MARKER, |ctx| {
            action(ctx)?;
            ctx.charge_work(1, WORK_MARKER)?;
            Ok(())
        });
    let CodecError::ResourceLimit(limit) = error else {
        panic!("work marker must refuse after the route completes: {error:?}");
    };
    assert_eq!(limit.operation, WORK_MARKER);
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.additional, 1);
    limit.used
}

fn with_unlimited_probe<T>(
    dimension: ResourceDimension,
    operation: &str,
    run: impl FnOnce(&DecodeContext<'_>) -> Result<T, CodecError>,
) -> Result<T, CodecError> {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    match dimension {
        ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = u64::MAX,
        ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = u64::MAX,
        ResourceDimension::CollectionItems => policy.limits.max_collection_items = u64::MAX,
        ResourceDimension::WorkUnits => policy.limits.max_work_units = u64::MAX,
        other => panic!("unsupported probe dimension {other:?}"),
    }
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is within the service policy");
    let _probe = RefusalProbe::arm(dimension, operation, None);
    let result = run(&ctx);
    assert_eq!(
        ctx.resource_refusal(),
        None,
        "unexpected {operation} charge"
    );
    result
}

fn with_pattern_sources<T>(
    ctx: &DecodeContext<'_>,
    objects: &[crate::native::ObjectRecord],
    features: &HashMap<&str, FeatureId>,
    properties_by_owner: &BTreeMap<&str, Vec<&crate::native::PropertyRecord>>,
    run: impl FnOnce(crate::design::PatternSources<'_, '_, '_, '_>) -> Result<T, CodecError>,
) -> Result<T, CodecError> {
    let object_by_id = crate::design::ObjectIndex::new(ctx, objects);
    let predecessors =
        crate::design::BodyPredecessors::new(ctx, objects, features, properties_by_owner);
    run(crate::design::PatternSources {
        objects,
        object_by_id: &object_by_id,
        predecessors: &predecessors,
        properties_by_owner,
        entries: &[],
    })
}

#[test]
fn sketch_placement_forwards_charged_reads_and_projects_one_validated_frame() {
    let object = object(
        "fcstd:native:object#Sketch",
        "Sketcher::SketchObject",
        0,
        Vec::new(),
    );
    let placement = placement_property(
        object.id(),
        &[
            ("Px", "3"),
            ("Py", "-2"),
            ("Pz", "5"),
            ("Q0", "0"),
            ("Q1", "0"),
            ("Q2", "1"),
            ("Q3", "0"),
        ],
    );
    let properties = [&placement];

    for operation in [
        "FreeCAD placement attribute lookup",
        "FreeCAD placement angle lookup",
        "FreeCAD placement number parse",
    ] {
        let error =
            crate::test_support::refusal_at(ResourceDimension::WorkUnits, &[], operation, |ctx| {
                super::super::validate_sketch_placement(ctx, &properties).map(drop)
            });
        assert!(matches!(error,
            CodecError::ResourceLimit(limit) if limit.operation == operation));
    }

    let expected = (
        Point3::new(3.0, -2.0, 5.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(-1.0, 0.0, 0.0),
        Vector3::new(0.0, -1.0, 0.0),
    );
    crate::test_support::with_service_context(&[], |ctx| {
        assert_eq!(
            super::super::validate_sketch_placement(ctx, &properties).expect("valid placement"),
            Some(expected)
        );
        let parsed =
            super::super::parse_sketch(ctx, &object, &properties).expect("valid sketch placement");
        let Some((origin, normal, u_axis)) = parsed.sketch.placement.resolved() else {
            panic!("placement should be resolved");
        };
        assert_eq!(origin.get(), expected.0);
        assert_eq!(normal.get(), expected.1);
        assert_eq!(u_axis.get(), expected.2);
    });

    let validated_work = work_used_before_marker(|ctx| {
        super::super::validate_sketch_placement(ctx, &properties).map(drop)
    });
    let projected_work =
        work_used_before_marker(|ctx| super::super::sketch_frame(ctx, &properties).map(drop));
    assert_eq!(projected_work, validated_work);
}

#[test]
fn constraint_integer_groups_have_linear_work_and_keep_malformed_groups_invalid() {
    const ITEMS: usize = 256;
    let values = std::iter::repeat_n("0", ITEMS)
        .collect::<Vec<_>>()
        .join(",");
    let used = work_used_before_marker(|ctx| {
        let parsed = super::super::split_ints(ctx, &values)?;
        assert_eq!(parsed.len(), ITEMS);
        assert!(parsed.iter().all(|value| *value == 0));
        Ok(())
    });
    // Each group has bounded scan and parse work. The allowance grows linearly
    // with token count and covers fixed parser overhead.
    assert!(used <= u64::try_from(64 * ITEMS + 64).expect("linear work cap fits"));

    for malformed in [",1", "1,", "1,,2", "1,not-an-integer"] {
        let error = crate::test_support::with_service_context(&[], |ctx| {
            super::super::split_ints(ctx, malformed).expect_err("malformed group is refused")
        });
        assert!(
            matches!(error, CodecError::Malformed(_)),
            "{malformed}: {error:?}"
        );
    }
}

#[test]
fn operation_parameters_reuse_owner_and_clone_only_emitted_values() {
    const MEASUREMENT_MARKER_BYTES: u64 = 1 << 20;
    let object = object(
        "fcstd:native:object#Feature",
        "Part::Feature",
        0,
        Vec::new(),
    );
    let owner = FeatureId::mint("fcstd:design:feature#Feature").expect("owner feature id");
    let valid = super::scalar_property(object.id(), "Length", "3");
    let malformed = super::scalar_property(object.id(), "Length", "not-a-number");
    let wrong_carrier = property(
        object.id(),
        "Length",
        "App::PropertyString",
        "<Property><String value=\"3\"/></Property>",
    );

    for dimension in [
        ResourceDimension::MaterializedBytes,
        ResourceDimension::RetainedBytes,
    ] {
        with_unlimited_probe(dimension, "fcstd design feature identity", |ctx| {
            let mut parameters = Vec::new();
            super::super::append_operation_parameters(ctx, &mut parameters, &object, &owner, &[])?;
            super::super::append_operation_parameters(
                ctx,
                &mut parameters,
                &object,
                &owner,
                &[&wrong_carrier],
            )?;
            assert!(parameters.is_empty());
            Ok(())
        })
        .expect("empty and wrong-carrier candidates do not construct an owner identity");
    }

    with_unlimited_probe(
        ResourceDimension::RetainedBytes,
        "fcstd operation scalar expression",
        |ctx| {
            let mut parameters = Vec::new();
            super::super::append_operation_parameters(
                ctx,
                &mut parameters,
                &object,
                &owner,
                &[&malformed],
            )?;
            assert!(parameters.is_empty());
            Ok(())
        },
    )
    .expect("nonnumeric candidates do not retain output expressions");

    let mut existing = single_parameter();
    existing.owner = Some(owner.clone());
    existing.name = "Width".to_owned();
    let owner_comparison_error = crate::test_support::refusal_at(
        ResourceDimension::WorkUnits,
        &[],
        "fcstd operation parameter owner",
        |ctx| {
            let mut parameters = vec![existing.clone()];
            super::super::append_operation_parameters(
                ctx,
                &mut parameters,
                &object,
                &owner,
                &[&valid],
            )
        },
    );
    assert!(matches!(owner_comparison_error,
        CodecError::ResourceLimit(limit)
            if limit.operation == "fcstd operation parameter owner"));

    let owner_output_error = crate::test_support::refusal_at(
        ResourceDimension::RetainedBytes,
        &[],
        "fcstd operation parameter owner",
        |ctx| {
            let mut parameters = Vec::new();
            super::super::append_operation_parameters(
                ctx,
                &mut parameters,
                &object,
                &owner,
                &[&valid],
            )
        },
    );
    assert!(matches!(owner_output_error,
        CodecError::ResourceLimit(limit)
            if limit.operation == "fcstd operation parameter owner"));

    let marker = "test operation owner scope release marker";
    let error = crate::test_support::materialized_refusal_at(marker, |ctx| {
        let mut parameters = Vec::new();
        super::super::append_operation_parameters(
            ctx,
            &mut parameters,
            &object,
            &owner,
            &[&valid],
        )?;
        assert_eq!(parameters.len(), 1);
        ctx.reserve_scoped(MEASUREMENT_MARKER_BYTES, marker)
            .map(drop)
    });
    assert!(matches!(error,
        CodecError::ResourceLimit(limit)
            if limit.operation == marker
                && limit.used == 0
                && limit.additional == MEASUREMENT_MARKER_BYTES));
}

#[test]
fn invalid_scaled_pattern_factor_drops_explicit_and_implicit_seed_candidates() {
    let originals = super::linked_property_count_to("pattern", "Originals", "originals", 1, "seed");
    let factor = super::scalar_property("pattern", "Factor", "0");
    let explicit_features = HashMap::from([(
        "seed",
        FeatureId::mint("test:test:feature#seed").expect("seed feature id"),
    )]);
    let no_properties = BTreeMap::new();
    let explicit = with_unlimited_probe(
        ResourceDimension::RetainedBytes,
        "fcstd pattern seed identity",
        |ctx| {
            with_pattern_sources(ctx, &[], &explicit_features, &no_properties, |sources| {
                crate::design::pattern_definition(
                    ctx,
                    "PartDesign::Scaled",
                    "pattern",
                    &[&originals, &factor],
                    &explicit_features,
                    sources,
                )
            })
        },
    )
    .expect("invalid factor is not a resource refusal");
    assert!(explicit.is_none());

    let body = object(
        "fcstd:native:object#Body",
        "PartDesign::Body",
        0,
        Vec::new(),
    );
    let seed = object("fcstd:native:object#Seed", "Part::Feature", 1, Vec::new());
    let stage = object(
        "fcstd:native:object#Scaled",
        "PartDesign::Scaled",
        2,
        Vec::new(),
    );
    let objects = vec![body, seed, stage];
    let body_group = membership_property(objects[0].id(), &[objects[1].id(), objects[2].id()]);
    let implicit_factor = super::scalar_property(objects[2].id(), "Factor", "0");
    let properties_by_owner = BTreeMap::from([
        (objects[0].id().as_str(), vec![&body_group]),
        (objects[2].id().as_str(), vec![&implicit_factor]),
    ]);
    let features = HashMap::from([
        (
            objects[1].id().as_str(),
            FeatureId::mint("test:test:feature#seed").expect("seed feature id"),
        ),
        (
            objects[2].id().as_str(),
            FeatureId::mint("test:test:feature#stage").expect("stage feature id"),
        ),
    ]);
    let implicit = with_unlimited_probe(
        ResourceDimension::RetainedBytes,
        "fcstd implicit pattern seed identity",
        |ctx| {
            with_pattern_sources(ctx, &objects, &features, &properties_by_owner, |sources| {
                crate::design::pattern_definition(
                    ctx,
                    "PartDesign::Scaled",
                    objects[2].id(),
                    &[&implicit_factor],
                    &features,
                    sources,
                )
            })
        },
    )
    .expect("invalid implicit factor is not a resource refusal");
    assert!(implicit.is_none());
}

#[test]
fn reference_horizontal_axis_is_neutral_and_native_unresolved_operands_keep_order() {
    let object = object(
        "fcstd:native:object#Sketch",
        "Sketcher::SketchObject",
        0,
        Vec::new(),
    );
    let property = constraint_property(
        object.id(),
        "<Property><ConstraintList count=\"1\"><Constrain Type=\"2\" First=\"-1\" FirstPos=\"0\"/></ConstraintList></Property>",
    );
    let sketch = SketchId::mint("fcstd:design:sketch#Sketch").expect("sketch id");
    let reference_id = cadmpeg_ir::sketches::SketchEntityId::mint(
        "fcstd:design:sketch-entity#Sketch:reference-horizontal-axis",
    )
    .expect("horizontal reference entity id");
    let reference_geometry = SketchGeometry::try_from(SketchGeometryDefinition::ReferenceLine {
        origin: Point2::new(0.0, 0.0),
        direction: Point2::new(1.0, 0.0),
    })
    .expect("valid horizontal reference line");
    let reference = SketchEntity::new(reference_id.clone(), sketch.clone(), reference_geometry);
    for operation in [
        "fcstd sketch constraint type",
        "fcstd native operand position kind",
    ] {
        with_unlimited_probe(ResourceDimension::RetainedBytes, operation, |ctx| {
            super::parse_constraints(
                ctx,
                &object,
                &[&property],
                &sketch,
                std::slice::from_ref(&reference),
            )
            .map(drop)
        })
        .unwrap_or_else(|error| {
            panic!("neutral horizontal constraint reached {operation}: {error:?}")
        });
    }
    let (constraints, parameters) = crate::test_support::with_service_context(&[], |ctx| {
        super::parse_constraints(ctx, &object, &[&property], &sketch, &[reference])
            .expect("reference horizontal constraint")
    });
    assert_eq!(constraints.len(), 1);
    assert!(parameters.is_empty());
    assert!(matches!(constraints[0].definition.kind(),
        SketchConstraintDefinitionInput::Horizontal { entity } if entity == &reference_id));

    let native_property = constraint_property(
        object.id(),
        "<Property><ConstraintList count=\"1\"><Constrain Type=\"99\" ElementIds=\"4 -1 0 2\" ElementPositions=\"1 0 1 3\"/></ConstraintList></Property>",
    );
    let line_id =
        cadmpeg_ir::sketches::SketchEntityId::mint("fcstd:design:sketch-entity#Sketch:line")
            .expect("line entity id");
    let line_geometry = SketchGeometry::try_from(SketchGeometryDefinition::Line {
        start: Point2::new(0.0, 0.0),
        end: Point2::new(2.0, 0.0),
    })
    .expect("valid line geometry");
    let line = SketchEntity::new(line_id, sketch.clone(), line_geometry);
    for operation in [
        "fcstd sketch constraint type",
        "fcstd native operand position kind",
    ] {
        let error = crate::test_support::refusal_at(
            ResourceDimension::RetainedBytes,
            &[],
            operation,
            |ctx| {
                super::parse_constraints(
                    ctx,
                    &object,
                    &[&native_property],
                    &sketch,
                    std::slice::from_ref(&line),
                )
                .map(drop)
            },
        );
        assert!(matches!(error,
            CodecError::ResourceLimit(limit) if limit.operation == operation));
    }
    let (constraints, _) = crate::test_support::with_service_context(&[], |ctx| {
        super::parse_constraints(ctx, &object, &[&native_property], &sketch, &[line])
            .expect("native unresolved operands")
    });
    let [constraint] = constraints.as_slice() else {
        panic!("one native constraint should be retained");
    };
    let SketchConstraintDefinitionInput::Native {
        entities, operands, ..
    } = constraint.definition.kind()
    else {
        panic!("unknown relation should remain native");
    };
    assert_eq!(entities.len(), 1);
    assert_eq!(
        operands
            .iter()
            .map(|operand| (operand.object_index, operand.native_kind.as_str()))
            .collect::<Vec<_>>(),
        vec![
            (Some(4), "position:1"),
            (None, "position:0"),
            (Some(2), "position:3"),
        ]
    );
}

#[test]
fn cyclic_sketch_keeps_neutral_side_outputs_while_its_feature_stays_native() {
    let id = "fcstd:native:object#Sketch";
    let object = object(id, "Sketcher::SketchObject", 0, vec![id.to_owned()]);
    let geometry = geometry_property(
        object.id(),
        "<Property><GeometryList count=\"1\"><Geometry type=\"Part::GeomLineSegment\" id=\"0\"><LineSegment StartX=\"0\" StartY=\"0\" EndX=\"4\" EndY=\"0\"/></Geometry></GeometryList></Property>",
    );
    let constraints = constraint_property(
        object.id(),
        "<Property><ConstraintList count=\"4\"><Constrain Type=\"6\" ElementIds=\"0\" ElementPositions=\"1\" Value=\"4\"/><Constrain Type=\"2\" First=\"-1\" FirstPos=\"0\"/><Constrain Type=\"3\" First=\"-2\" FirstPos=\"0\"/><Constrain Type=\"7\" ElementIds=\"-1\" ElementPositions=\"1\"/></ConstraintList></Property>",
    );
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    crate::test_support::with_service_context(&[], |ctx| {
        super::super::transfer(
            ctx,
            &mut ir,
            std::slice::from_ref(&object),
            &[geometry, constraints],
            &[],
            &[],
            None,
        )
        .expect("cyclic sketch transfer keeps supported side outputs");
    });

    assert_eq!(ir.model.sketches.len(), 1);
    assert_eq!(ir.model.sketch_entities.len(), 4);
    for suffix in [
        ":reference-horizontal-axis",
        ":reference-vertical-axis",
        ":reference-root-point",
    ] {
        assert!(ir
            .model
            .sketch_entities
            .iter()
            .any(|entity| entity.id().as_str().ends_with(suffix)));
    }
    assert_eq!(ir.model.sketch_constraints.len(), 4);
    assert_eq!(ir.model.parameters.len(), 1);
    assert_eq!(ir.model.parameters[0].name, "Constraint1");
    assert_eq!(ir.model.parameters[0].expression, "4");
    let [feature] = ir.model.features.as_slice() else {
        panic!("cyclic sketch should still have one feature record");
    };
    assert!(feature.dependencies.is_empty());
    assert!(matches!(feature.evaluation.definition(),
        FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::Native { kind, .. })
            if kind.as_str() == "Sketcher::SketchObject"));
    assert!(matches!(
        ir.model.sketch_constraints[0].definition.kind(),
        SketchConstraintDefinitionInput::Distance { .. }
    ));
    assert!(ir.model.sketch_constraints.iter().any(|constraint| {
        matches!(
            constraint.definition.kind(),
            SketchConstraintDefinitionInput::Horizontal { .. }
        )
    }));
    assert!(ir.model.sketch_constraints.iter().any(|constraint| {
        matches!(
            constraint.definition.kind(),
            SketchConstraintDefinitionInput::Vertical { .. }
        )
    }));
}

#[test]
fn self_cyclic_spreadsheet_keeps_cell_projection_while_its_feature_stays_native() {
    let id = "fcstd:native:object#Sheet";
    let object = object(id, "Spreadsheet::Sheet", 0, vec![id.to_owned()]);
    let cells = property(
        object.id(),
        "cells",
        "Spreadsheet::PropertySheet",
        "<Property><Cells Count=\"1\"><Cell address=\"A1\" content=\"5\" alias=\"width\"/></Cells></Property>",
    );
    let objects = [object];
    let properties = [cells];
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    crate::test_support::with_service_context(&[], |ctx| {
        super::super::transfer(ctx, &mut ir, &objects, &properties, &[], &[], None)
            .expect("self-cyclic sheet transfer keeps cell side outputs");
    });

    let [sheet] = ir.model.spreadsheets.as_slice() else {
        panic!("self-cyclic sheet should retain one spreadsheet");
    };
    assert_eq!(sheet.feature.as_str(), "fcstd:design:feature#Sheet");
    let [cell] = sheet.cells() else {
        panic!("self-cyclic sheet should retain its one used cell");
    };
    assert_eq!(cell.address.a1(), "A1");

    let [feature] = ir.model.features.as_slice() else {
        panic!("self-cyclic sheet should retain one feature record");
    };
    assert!(feature.dependencies.is_empty());
    assert!(matches!(feature.evaluation.definition(),
        FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::Native { kind, .. })
            if kind.as_str() == "Spreadsheet::Sheet"));

    let [parameter] = ir.model.parameters.as_slice() else {
        panic!("self-cyclic sheet should project one cell parameter");
    };
    assert_eq!(parameter.owner.as_ref(), Some(&feature.id));
    assert_eq!(parameter.name, "width");
    assert_eq!(parameter.expression, "5");
    assert_eq!(
        parameter.value,
        Some(cadmpeg_ir::features::ParameterValue::Real(
            cadmpeg_ir::scalar::FiniteReal::new(5.0).expect("finite cell value"),
        ))
    );
    assert!(parameter.dependencies.is_empty());
    assert_eq!(cell.parameter, parameter.id);
}

#[test]
fn cyclic_box_and_cut_skip_discarded_primitive_work_and_keep_parameters() {
    let box_id = "fcstd:native:object#Box";
    let cut_id = "fcstd:native:object#Cut";
    let box_object = object(
        "fcstd:native:object#Box",
        "Part::Box",
        0,
        vec![cut_id.into()],
    );
    let cut_object = object(
        "fcstd:native:object#Cut",
        "Part::Cut",
        1,
        vec![box_id.into()],
    );
    let box_properties = [
        super::scalar_property(box_object.id(), "Length", "10"),
        super::scalar_property(box_object.id(), "Width", "20"),
        super::scalar_property(box_object.id(), "Height", "30"),
    ];
    let cut_length = super::scalar_property(cut_object.id(), "Length", "4");
    let objects = [box_object, cut_object];
    let properties = [
        box_properties[0].clone(),
        box_properties[1].clone(),
        box_properties[2].clone(),
        cut_length,
    ];
    let scalar_value_error = crate::test_support::refusal_at(
        ResourceDimension::WorkUnits,
        &[],
        "fcstd scalar property value",
        |ctx| {
            super::super::primitive_definition(
                ctx,
                "Part::Box",
                &[&box_properties[0], &box_properties[1], &box_properties[2]],
            )
            .map(drop)
        },
    );
    assert!(matches!(scalar_value_error,
        CodecError::ResourceLimit(limit) if limit.operation == "fcstd scalar property value"));

    let mut ir = cadmpeg_ir::document::CadIr::empty();
    with_unlimited_probe(
        ResourceDimension::WorkUnits,
        "fcstd scalar property value",
        |ctx| super::super::transfer(ctx, &mut ir, &objects, &properties, &[], &[], None).map(drop),
    )
    .expect("cyclic native definitions skip primitive parsing");

    assert_eq!(ir.model.features.len(), 2);
    for (feature, expected_kind) in ir.model.features.iter().zip(["Part::Box", "Part::Cut"]) {
        assert!(feature.dependencies.is_empty());
        assert!(matches!(feature.evaluation.definition(),
            FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::Native { kind, .. })
                if kind.as_str() == expected_kind));
    }
    let box_feature = &ir.model.features[0];
    assert!(ir.model.parameters.iter().any(|parameter| {
        parameter.owner.as_ref() == Some(&box_feature.id) && parameter.name == "Length"
    }));
    assert!(!ir.model.parameters.iter().any(|parameter| {
        parameter.owner.as_ref() == Some(&box_feature.id)
            && matches!(parameter.name.as_str(), "Width" | "Height")
    }));
    assert!(ir.model.parameters.iter().any(|parameter| {
        parameter.owner.as_ref() == Some(&ir.model.features[1].id) && parameter.name == "Length"
    }));
}

#[test]
fn unused_transfer_indexes_stay_unbuilt_without_their_consumers() {
    let body = object(
        "fcstd:native:object#Body",
        "PartDesign::Body",
        0,
        Vec::new(),
    );
    let first = object("fcstd:native:object#First", "Part::Feature", 1, Vec::new());
    let second = object("fcstd:native:object#Second", "Part::Feature", 2, Vec::new());
    let group = membership_property(body.id(), &[first.id(), second.id()]);
    let objects = [body, first, second];
    let properties = [group];

    for operation in [
        "fcstd design object index",
        "fcstd design property identity index",
        "fcstd body predecessor objects",
        "fcstd design body output prefix",
        "fcstd external link index",
    ] {
        with_unlimited_probe(ResourceDimension::WorkUnits, operation, |ctx| {
            let mut ir = cadmpeg_ir::document::CadIr::empty();
            super::super::transfer(ctx, &mut ir, &objects, &properties, &[], &[], None).map(drop)
        })
        .unwrap_or_else(|error| panic!("unused {operation} should not be charged: {error:?}"));
    }
}

#[test]
fn body_output_separator_work_does_not_scale_with_the_key_tail() {
    let make_payload = |id: String| crate::brep::ShapePayloadRecord {
        id,
        property: "fcstd:native:property#Body:Shape".to_owned(),
        entry: "shape.brp".to_owned(),
        payload: crate::brep::ShapePayload::Empty,
    };
    let short = make_payload("fcstd:native:shape-payload#Body".to_owned());
    let long = make_payload(format!(
        "fcstd:native:shape-payload#Body{}",
        "unread-tail".repeat(512)
    ));
    let work_before_prefix_format = |payload: &crate::brep::ShapePayloadRecord| {
        let error = crate::test_support::refusal_at(
            ResourceDimension::WorkUnits,
            &[],
            "fcstd design body output prefix",
            |ctx| super::super::BodyOutputPrefix::new(ctx, payload).map(drop),
        );
        let CodecError::ResourceLimit(limit) = error else {
            panic!("prefix formatting must hit its positive work boundary");
        };
        assert_eq!(limit.operation, "fcstd design body output prefix");
        limit.used
    };
    let short_work = work_before_prefix_format(&short);
    let long_work = work_before_prefix_format(&long);
    assert_eq!(short_work, long_work);
}

fn single_parameter() -> DesignParameter {
    DesignParameter {
        id: cadmpeg_ir::features::ParameterId::mint("test:test:parameter#single")
            .expect("parameter id"),
        owner: Some(FeatureId::mint("test:test:feature#single").expect("feature id")),
        ordinal: 0,
        name: "Length".to_owned(),
        expression: "1".to_owned(),
        display: None,
        value: None,
        dependencies: DistinctMembers::default(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    }
}

fn one_profile_sweep(ctx: &DecodeContext<'_>) -> Result<Option<FeatureDefinition>, CodecError> {
    let profile = super::linked_property("sweep", "Profile", "profile-property");
    let path = super::linked_property("sweep", "Spine", "path-property");
    super::super::sweep_definition(ctx, "Part::Sweep", &[&profile, &path], &HashMap::new())
}

#[test]
fn single_parameter_and_sweep_skip_rotation_and_profile_deduplication() {
    with_unlimited_probe(
        ResourceDimension::WorkUnits,
        "fcstd parameter dependency extraction",
        |ctx| {
            let mut parameters = vec![single_parameter()];
            let original = parameters[0].id.clone();
            super::super::ordering::order_parameters_by_dependencies(ctx, &mut parameters)
                .map(drop)?;
            assert_eq!(parameters.len(), 1);
            assert_eq!(parameters[0].id, original);
            Ok(())
        },
    )
    .expect("one ready parameter needs no rotation");

    for operation in [
        "fcstd sweep profile deduplication",
        "fcstd sweep primary profile removal",
    ] {
        let definition =
            with_unlimited_probe(ResourceDimension::WorkUnits, operation, one_profile_sweep)
                .expect("one sweep profile is admitted without movement");
        assert!(matches!(definition,
            Some(FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::Sweep {
                path: Some(cadmpeg_ir::features::PathRef::Native(path)),
                ..
            })) if path == "path-property"));
    }
}

#[test]
fn constraint_boolean_attributes_use_the_shared_native_boolean_grammar() {
    for value in ["true", "TRUE", "False", "1", "0", "yes", "True "] {
        let xml = format!("<Constraint Flag=\"{value}\"/>");
        let document = roxmltree::Document::parse(&xml).expect("valid test XML");
        let expected = crate::native::parse_bool(value);
        crate::test_support::with_service_context(&[], |ctx| {
            assert_eq!(
                super::super::bool_attr(ctx, document.root_element(), "Flag")
                    .expect("read boolean attribute"),
                expected,
                "{value:?}"
            );
        });
    }
}

#[test]
fn cyclic_extrusion_preserves_taper_validation_and_side_selection() {
    let object = object(
        "fcstd:native:object#Pad",
        "PartDesign::Pad",
        0,
        vec!["fcstd:native:object#Pad".into()],
    );
    for (key, side, malformed) in [
        ("TaperAngle", "0", true),
        ("TaperAngleRev", "0", true),
        ("TaperAngle2", "1", true),
        ("TaperAngle2", "0", false),
    ] {
        let taper = super::scalar_property(object.id(), key, "90");
        let side = property(
            object.id(),
            "SideType",
            "App::PropertyEnumeration",
            format!("<Property><Integer value=\"{side}\"/></Property>"),
        );
        crate::test_support::with_service_context(&[], |ctx| {
            let mut ir = cadmpeg_ir::document::CadIr::empty();
            let result = super::super::transfer(
                ctx,
                &mut ir,
                std::slice::from_ref(&object),
                &[taper.clone(), side.clone()],
                &[],
                &[],
                None,
            );
            if malformed {
                assert!(
                    matches!(result, Err(CodecError::Malformed(ref message)) if message.starts_with(&format!("{key}: ")))
                );
            } else {
                let (cycles, _storage) = result.expect("unused second-side draft");
                assert!(cycles.contains(object.id().as_str()));
                assert!(matches!(
                    ir.model.features[0].evaluation.definition(),
                    FeatureDefinition::Operation(
                        cadmpeg_ir::features::FeatureOperation::Native { .. }
                    )
                ));
            }
        });
    }
}

#[test]
fn cyclic_extrusion_preserves_profile_placement_before_taper_error() {
    let pad = object(
        "fcstd:native:object#Pad",
        "PartDesign::Pad",
        0,
        vec!["fcstd:native:object#Pad".into()],
    );
    let profile = object(
        "fcstd:native:object#Profile",
        "Sketcher::SketchObject",
        1,
        Vec::new(),
    );
    let mut link = membership_property(pad.id(), &[profile.id()]);
    link.name = "Profile".into();
    link.type_name = "App::PropertyLink".into();
    let placement = placement_property(
        profile.id(),
        &[
            ("Px", "0"),
            ("Py", "0"),
            ("Pz", "0"),
            ("Q0", "0"),
            ("Q1", "0"),
            ("Q2", "0"),
            ("Q3", "0"),
        ],
    );
    let taper = super::scalar_property(pad.id(), "TaperAngle", "90");
    crate::test_support::with_service_context(&[], |ctx| {
        let mut ir = cadmpeg_ir::document::CadIr::empty();
        let error = super::super::transfer(
            ctx,
            &mut ir,
            &[pad, profile],
            &[link, placement, taper],
            &[],
            &[],
            None,
        )
        .expect_err("placement must be validated");
        assert!(
            matches!(error, CodecError::Malformed(ref message) if message == "sketch Placement placement carrier has incomplete or invalid components")
        );
    });
}

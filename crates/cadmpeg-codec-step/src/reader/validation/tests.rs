// SPDX-License-Identifier: Apache-2.0
//! STEP geometric validation-property tests.

#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::StepCodec;

const VALIDATION_LIMIT_SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=PROPERTY_DEFINITION('geometric validation property','description',$);#2=REPRESENTATION('unused',(),$);#3=PROPERTY_DEFINITION_REPRESENTATION(#1,#2);ENDSEC;END-ISO-10303-21;";
const VALIDATION_COLLECTION_SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=PROPERTY_DEFINITION('geometric validation property','description',$);#2=REPRESENTATION('unused',(#4),$);#3=PROPERTY_DEFINITION_REPRESENTATION(#1,#2);#4=CARTESIAN_POINT('point',(1.,2.,3.));#5=ITEM(#4);ENDSEC;END-ISO-10303-21;";
const VALIDATION_UNSUPPORTED_SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=PROPERTY_DEFINITION('geometric validation property','description',$);#2=REPRESENTATION('unused',(#4),$);#3=PROPERTY_DEFINITION_REPRESENTATION(#1,#2);#4=ITEM();ENDSEC;END-ISO-10303-21;";

fn validation_resource_refuses(
    source: &[u8],
    operation: &str,
    dimension: cadmpeg_core::decode::ResourceDimension,
) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid validation collection source");
    let setup_arena = DecodeArena::new();
    let setup_policy = DecodePolicy::service();
    let (setup_ctx, _) = DecodeContext::from_root_bytes(source, &setup_arena, &setup_policy)
        .expect("root fits setup policy");
    let mut setup_ir = cadmpeg_ir::document::CadIr::empty();
    let geometry = crate::reader::geometry::decode(&exchange, &mut setup_ir, &setup_ctx)
        .expect("geometry setup decodes");
    let refused = {
        let error =
            cadmpeg_test_support::refusal::resource_limit_at(dimension, operation, |limit| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                match dimension {
                    ResourceDimension::CollectionItems => {
                        policy.limits.max_collection_items = limit;
                    }
                    ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = limit,
                    _ => unreachable!("test only selects collection or retained limits"),
                }
                let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
                    .expect("root fits selected policy");
                let mut ir = setup_ir.clone();
                (super::decode(&exchange, &geometry.value, &mut ir, &ctx)).map(|_| ())
            });
        matches!(Err::<(), CodecError>(error), Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == dimension
                    && refusal.operation == operation)
    };
    assert!(refused, "no {dimension:?} limit refused {operation}");
}

macro_rules! validation_collection_test {
    ($name:ident, $operation:literal) => {
        #[test]
        fn $name() {
            validation_resource_refuses(
                VALIDATION_COLLECTION_SOURCE,
                $operation,
                cadmpeg_core::decode::ResourceDimension::CollectionItems,
            );
        }
    };
}

validation_collection_test!(
    validation_representation_items_refuse_collection_limit,
    "step_validation_representation_items"
);
validation_collection_test!(
    validation_representations_refuse_collection_limit,
    "step_validation_representations"
);
validation_collection_test!(
    validation_used_representations_refuse_collection_limit,
    "step_validation_used_representations"
);
validation_collection_test!(
    validation_points_refuse_collection_limit,
    "step_validation_points"
);
validation_collection_test!(
    validation_claims_refuse_collection_limit,
    "step_validation_claims"
);
validation_collection_test!(
    validation_referenced_points_refuse_collection_limit,
    "step_validation_referenced_points"
);
validation_collection_test!(
    validation_notes_refuse_collection_limit,
    "step_validation_notes"
);

#[test]
fn validation_note_text_refuses_retained_limit() {
    validation_resource_refuses(
        VALIDATION_COLLECTION_SOURCE,
        "step_validation_note_text",
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
    );
}

#[test]
fn validation_losses_refuse_collection_limit() {
    validation_resource_refuses(
        VALIDATION_UNSUPPORTED_SOURCE,
        "step_validation_losses",
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
    );
}

fn validation_limit_result(
    retained_limit: Option<u64>,
    collection_limit: Option<u64>,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let (exchange, _) = crate::test_support::with_service_context(
        VALIDATION_LIMIT_SOURCE,
        crate::parse::parse_inner,
    )
    .expect("valid validation-property exchange");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    if let Some(limit) = retained_limit {
        policy.limits.max_retained_bytes = limit;
    }
    if let Some(limit) = collection_limit {
        policy.limits.max_collection_items = limit;
    }
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let geometry_policy = DecodePolicy::service();
    let (geometry_ctx, _) =
        DecodeContext::from_root_bytes(VALIDATION_LIMIT_SOURCE, &arena, &geometry_policy)
            .expect("root fits geometry policy");
    let geometry = crate::reader::geometry::decode(&exchange, &mut ir, &geometry_ctx)?;
    let (ctx, _) = DecodeContext::from_root_bytes(VALIDATION_LIMIT_SOURCE, &arena, &policy)
        .expect("root fits selected policy");
    super::decode(&exchange, &geometry.value, &mut ir, &ctx)?;
    Ok(())
}

#[test]
fn validation_property_name_refuses_materialized_limit() {
    validation_property_text_refuses(None);
}

#[test]
fn validation_property_description_refuses_materialized_limit() {
    validation_property_text_refuses(Some(cadmpeg_core::decode::u64_from_index(
        "description".len(),
    )));
}

fn validation_property_text_refuses(text_bytes: Option<u64>) {
    use cadmpeg_core::decode::refusal_probe::RefusalProbe;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let (exchange, _) = crate::test_support::with_service_context(
        VALIDATION_LIMIT_SOURCE,
        crate::parse::parse_inner,
    )
    .expect("exchange");
    let setup = cadmpeg_test_support::service_decode_context();
    let mut ir = cadmpeg_ir::CadIr::empty();
    let geometry = crate::reader::geometry::decode(&exchange, &mut ir, &setup).expect("geometry");
    // Source bytes and prior scratch allocations precede the selected string.
    let probe = RefusalProbe::arm(
        ResourceDimension::MaterializedBytes,
        "step_string_text",
        text_bytes,
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = u64::MAX;
    let (ctx, _) =
        DecodeContext::from_root_bytes(VALIDATION_LIMIT_SOURCE, &arena, &policy).expect("root");
    let Err(CodecError::ResourceLimit(refusal)) =
        super::decode(&exchange, &geometry.value, &mut ir.clone(), &ctx)
    else {
        panic!("selected string boundary");
    };
    assert_eq!(refusal.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(refusal.operation, "step_string_text");
    drop(probe);
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = refusal.used + refusal.additional - 1;
    let (ctx, _) =
        DecodeContext::from_root_bytes(VALIDATION_LIMIT_SOURCE, &arena, &policy).expect("root");
    let Err(CodecError::ResourceLimit(replay)) =
        super::decode(&exchange, &geometry.value, &mut ir, &ctx)
    else {
        panic!("string replay refusal");
    };
    assert_eq!(
        (
            replay.dimension,
            replay.operation,
            replay.used,
            replay.additional
        ),
        (
            refusal.dimension,
            refusal.operation,
            refusal.used,
            refusal.additional
        )
    );
    assert_eq!(ctx.resource_refusal(), Some(replay));
}

#[test]
fn validation_property_map_refuses_collection_limit() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;

    assert!(matches!(
        validation_limit_result(None, Some(0)),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_validation_properties"
    ));
}

#[test]
fn validation_mesh_edges_refuse_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let source = include_bytes!("../../../tests/fixtures/ap242_tessellation.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("mesh validation source decodes");
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::CollectionItems,
        "step_validation_mesh_edges",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
                .expect("root fits collection policy");
            super::mesh_properties(result.ir(), &ctx)
        },
    );
}

#[test]
fn validation_mesh_triangles_refuse_work_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let source = include_bytes!("../../../tests/fixtures/ap242_tessellation.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("mesh validation source decodes");
    // The mesh scan and body matching charge work before the first triangle,
    // so the boundary is found by probing the triangle charge itself.
    let refusal = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "step_validation_mesh_triangles",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
                .expect("root fits work policy");
            super::mesh_properties(result.ir(), &ctx)
        },
    );
    assert!(matches!(
        refusal,
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::WorkUnits
                && refusal.operation == "step_validation_mesh_triangles"
    ));
}

#[test]
fn complex_validation_measure_carrier_is_decoded() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_tessellation.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#42=REPRESENTATION('surface area',(#43),#2);",
        "#42=(REPRESENTATION('surface area',(#43),#2) SHAPE_REPRESENTATION());",
    )
    .replace(
        "#43=MEASURE_REPRESENTATION_ITEM('surface area measure',AREA_MEASURE(50.),#44);",
        "#43=(MEASURE_REPRESENTATION_ITEM() MEASURE_WITH_UNIT(AREA_MEASURE(50.),#44) REPRESENTATION_ITEM('surface area measure'));",
    );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode complex validation measure");

    assert!(result.report().notes.iter().any(|note| {
        note == "geometric validation surface area triangle sheet: expected 50, tessellation approximation 50"
    }));
    assert!(!result.report().losses.iter().any(|loss| {
        loss.message
            .contains("geometric validation property #41 has an unsupported value")
    }));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn direct_area_and_volume_unit_subtypes_scale_validation_measures() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_tessellation.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace("#44=DERIVED_UNIT((#55));", "#44=AREA_UNIT((#55));")
    .replace("#53=DERIVED_UNIT((#56));", "#53=VOLUME_UNIT((#56));");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode direct validation unit subtypes");

    assert!(result.report().notes.iter().any(|note| {
        note == "geometric validation surface area triangle sheet: expected 50, tessellation approximation 50"
    }));
    assert!(result.report().notes.iter().any(|note| {
        note == "geometric validation volume open sheet volume: expected 0, tessellation approximation 0"
    }));
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| { loss.message.contains("unit scale did not resolve") }));
    let unknowns = result
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown records");
    for id in [44, 53, 55, 56] {
        assert!(
            !unknowns
                .iter()
                .any(|record| record.id.as_str().ends_with(&format!("#{id}"))),
            "validation unit carrier #{id} was not typed"
        );
    }
}

#[test]
fn validation_representation_decodes_all_measure_items() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_tessellation.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#42=REPRESENTATION('surface area',(#43),#2);",
        "#42=REPRESENTATION('surface area',(#43,#52),#2);",
    );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode validation representation with multiple items");

    assert!(result.report().notes.iter().any(|note| {
        note == "geometric validation surface area triangle sheet: expected 50, tessellation approximation 50"
    }));
    assert!(result.report().notes.iter().any(|note| {
        note == "geometric validation volume triangle sheet: expected 0, tessellation approximation 0"
    }));
    assert!(!result.report().losses.iter().any(|loss| {
        loss.message
            .contains("geometric validation property #41 has unsupported item")
    }));
}

#[test]
fn ps06_combined_validation_items_ignore_set_order_and_keep_siblings() {
    let decode_fixture = |bytes: &[u8]| {
        StepCodec::default()
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("decode PS-06 fixture")
    };
    let validation_notes = |result: &cadmpeg_ir::codec::DecodeResult| {
        let mut notes = result
            .report()
            .notes
            .iter()
            .filter(|note| note.starts_with("geometric validation "))
            .cloned()
            .collect::<Vec<_>>();
        notes.sort();
        notes
    };
    let unsupported_items = |result: &cadmpeg_ir::codec::DecodeResult| {
        result
            .report()
            .losses
            .iter()
            .filter(|loss| {
                loss.message
                    .contains("geometric validation property #41 has unsupported item")
            })
            .map(|loss| loss.message.clone())
            .collect::<Vec<_>>()
    };

    let source_order = decode_fixture(include_bytes!(
        "tests/data/ps06_combined_validation_properties_source_order.p21"
    ));
    let reordered = decode_fixture(include_bytes!(
        "tests/data/ps06_combined_validation_properties_reordered.p21"
    ));

    let expected_notes = vec![
        "geometric validation centroid combined validation: expected (1,2,3)".to_owned(),
        "geometric validation surface area combined validation: expected 50".to_owned(),
        "geometric validation volume combined validation: expected 8".to_owned(),
    ];
    assert_eq!(validation_notes(&source_order), expected_notes);
    assert_eq!(validation_notes(&reordered), expected_notes);
    assert_eq!(
        unsupported_items(&source_order),
        ["geometric validation property #41 has unsupported item #60"]
    );
    assert_eq!(
        unsupported_items(&source_order),
        unsupported_items(&reordered)
    );
}

#[test]
fn validation_shape_representation_with_parameters_uses_inherited_items() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_tessellation.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#42=REPRESENTATION('surface area',(#43),#2);",
        "#42=SHAPE_REPRESENTATION_WITH_PARAMETERS('surface area',(#43),#2);",
    );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode parameterized validation representation");

    assert!(result.report().notes.iter().any(|note| {
        note == "geometric validation surface area triangle sheet: expected 50, tessellation approximation 50"
    }));
    assert!(!result.report().losses.iter().any(|loss| {
        loss.message
            .contains("geometric validation property #41 has unsupported item")
    }));
}

#[test]
fn numerical_followup_mesh_volume_and_centroid_are_translation_invariant() {
    use cadmpeg_ir::{
        math::Point3,
        tessellation::{Tessellation, TessellationMesh},
    };
    let source = include_bytes!("../../../tests/fixtures/ap242_tessellation.p21");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut ir = decoded.ir().clone();
    assert_eq!(ir.model.bodies.len(), 1);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(source, &arena, &policy)
        .expect("root fits selected policy");
    for offset in [0.0, 1e6, 1e9] {
        let points = [[0., 0., 0.], [2., 0., 0.], [0., 1., 0.], [0., 0., 1.]]
            .map(|p| Point3::new(p[0] + offset, p[1] + offset, p[2] + offset))
            .to_vec();
        let mesh = TessellationMesh::from_list_lanes(
            points,
            vec![[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]],
            None,
        )
        .unwrap();
        let mut tessellation = Tessellation::new(
            cadmpeg_ir::tessellation::TessellationId::mint(
                "test:step:tessellation#numerical-followup",
            )
            .expect("valid identity"),
            mesh,
            Vec::new(),
        )
        .unwrap();
        tessellation.body = Some(ir.model.bodies[0].id.clone());
        ir.model.tessellations = vec![tessellation];
        let properties = super::mesh_properties(&ir, &ctx)
            .expect("mesh budget admits calculation")
            .expect("mesh properties exist");
        assert!((properties.volume - 1.0 / 3.0).abs() <= f64::EPSILON);
        assert_eq!(
            properties.centroid,
            Point3::new(offset + 0.5, offset + 0.25, offset + 0.25)
        );
    }
}

#[test]
fn numerical_seventh_mesh_mass_properties_preserve_uniform_scale() {
    use cadmpeg_ir::{
        math::Point3,
        tessellation::{Tessellation, TessellationMesh},
    };
    let source = include_bytes!("../../../tests/fixtures/ap242_tessellation.p21");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut ir = decoded.ir().clone();
    assert_eq!(ir.model.bodies.len(), 1);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(source, &arena, &policy)
        .expect("root fits selected policy");
    for scale in [1.0e-90, 1.0, 1.0e90] {
        let points = [[0., 0., 0.], [2., 0., 0.], [0., 1., 0.], [0., 0., 1.]]
            .map(|p| Point3::new(p[0] * scale, p[1] * scale, p[2] * scale))
            .to_vec();
        let mesh = TessellationMesh::from_list_lanes(
            points,
            vec![[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]],
            None,
        )
        .unwrap();
        let mut tessellation = Tessellation::new(
            cadmpeg_ir::tessellation::TessellationId::mint(
                "test:step:tessellation#numerical-followup",
            )
            .expect("valid identity"),
            mesh,
            Vec::new(),
        )
        .unwrap();
        tessellation.body = Some(ir.model.bodies[0].id.clone());
        ir.model.tessellations = vec![tessellation];
        let properties = super::mesh_properties(&ir, &ctx)
            .expect("mesh budget admits calculation")
            .expect("mesh properties exist");
        assert!(
            (properties.volume / scale / scale / scale - 1.0 / 3.0).abs() <= 8.0 * f64::EPSILON
        );
        for (actual, expected) in [
            (properties.centroid.x / scale, 0.5),
            (properties.centroid.y / scale, 0.25),
            (properties.centroid.z / scale, 0.25),
        ] {
            assert!((actual - expected).abs() <= 8.0 * f64::EPSILON);
        }
    }
}

mod equality;

#[test]
fn validation_point_number_parse_preserves_refusal() {
    // Identity splitting reads the source text before the numeric parse.
    // The ladder admits that split and refuses the parse one unit below its need.
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "STEP validation point number parse",
        |limit| {
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_work_units = limit;
            let (ctx, _) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
            let result = super::step_id(&ctx, "step:data:point#7");
            if let Err(cadmpeg_core::CodecError::ResourceLimit(ref refusal)) = result {
                assert_eq!(ctx.resource_refusal(), Some(*refusal));
            }
            result
        },
    );
    let cadmpeg_core::CodecError::ResourceLimit(refusal) = error else {
        panic!("numeric parse must preserve the refusal");
    };
    assert_eq!(refusal.operation, "STEP validation point number parse");
}

#[test]
fn typed_omitted_descent_refuses_work_limit() {
    use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
    let value = crate::parse::Value::Typed("WRAP".into(), Box::new(crate::parse::Value::Omitted));
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "STEP typed validation measure descent",
        |limit| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = limit;
            crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
                super::area_or_volume_measure(ctx, &value).map(|_| ())
            })
        },
    );
}

#[test]
fn typed_validation_reference_descent_refuses_work_limit() {
    use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
    let value = crate::parse::Value::Typed("WRAP".into(), Box::new(crate::parse::Value::Omitted));
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "STEP typed validation reference descent",
        |limit| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = limit;
            crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
                super::collect_validation_references(
                    &value,
                    &std::collections::BTreeSet::new(),
                    &mut std::collections::BTreeSet::new(),
                    ctx,
                )
            })
        },
    );
}


#[test]
fn validation_representation_items_refuse_only_first_visit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let items = vec!["#4"; 1024].join(",");
    let source = std::str::from_utf8(VALIDATION_COLLECTION_SOURCE).unwrap()
        .replace("'unused',(#4)", &format!("'unused',({items})"));
    let (exchange, _) = crate::test_support::with_service_context(
        source.as_bytes(), crate::parse::parse_inner,
    ).expect("validation exchange with a long item list");
    let setup_arena = DecodeArena::new();
    let setup_policy = DecodePolicy::service();
    let (setup_ctx, _) = DecodeContext::from_root_bytes(
        source.as_bytes(), &setup_arena, &setup_policy,
    ).unwrap();
    assert_eq!(super::super::representation::item_values(
        &setup_ctx, exchange.records().get(&2).expect("representation record"),
    ).unwrap().unwrap().len(), 1024);
    let mut setup_ir = cadmpeg_ir::document::CadIr::empty();
    let geometry = crate::reader::geometry::decode(&exchange, &mut setup_ir, &setup_ctx)
        .expect("geometry setup");
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits, "STEP representation item traversal", |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy).unwrap();
            let mut ir = setup_ir.clone();
            let error = super::decode(&exchange, &geometry.value, &mut ir, &ctx)
                .err().expect("refuse the first representation item visit");
            let CodecError::ResourceLimit(refusal) = error else {
                panic!("representation item resource refusal");
            };
            assert_eq!(refusal.additional, 1);
            assert_eq!(ctx.resource_refusal(), Some(refusal));
            assert!(matches!(ctx.finish_session(),
                Err(CodecError::ResourceLimit(sticky)) if sticky == refusal));
            Err::<(), _>(CodecError::ResourceLimit(refusal))
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(refusal)
        if refusal.operation == "STEP representation item traversal" && refusal.additional == 1));
}

#[test]
fn validation_reference_list_refuses_only_first_visit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let value = crate::parse::Value::List(vec![crate::parse::Value::Reference(7); 1024]);
    let validation_points = std::collections::BTreeSet::from([7]);
    let mut referenced = std::collections::BTreeSet::new();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
    let error = super::collect_validation_references(
        &value, &validation_points, &mut referenced, &ctx,
    ).expect_err("only the first list visit is refused");
    let CodecError::ResourceLimit(refusal) = error else {
        panic!("validation list resource refusal");
    };
    assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
    assert_eq!(refusal.operation, "STEP collect validation references value traversal");
    assert_eq!(refusal.used, 0);
    assert_eq!(refusal.additional, 1);
    assert!(referenced.is_empty());
    assert_eq!(ctx.resource_refusal(), Some(refusal));
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(sticky)) if sticky == refusal));
}

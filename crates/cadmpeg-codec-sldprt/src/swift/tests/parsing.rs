use super::dimension_nominal;
use super::length;
use super::semantic_root;
use crate::swift::parse_unique_root;
use crate::swift::pmi_id;
use crate::swift::project;
use crate::swift::project_lower_profile_tier;
use crate::swift::Entity;
use crate::swift::ObjectSection;
use crate::swift::Reference;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::pmi::PmiDefinition;
use cadmpeg_ir::pmi::PmiQuantity;
use cadmpeg_ir::pmi::PmiTarget;
use cadmpeg_ir::pmi::PmiValue;
use std::collections::BTreeMap;
use std::collections::BTreeSet;

#[test]
fn lower_profile_tier_holds_an_admitted_nonnegative_magnitude() {
    let reference = Reference {
        id: "A42".into(),
        class: "GdtSurfaceProfile".into(),
    };
    let mut entity = Entity::default();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("empty test root fits policy");
    entity.doubles.insert("ToleranceLowerTier".into(), 0.0);
    let projected = project_lower_profile_tier(&ctx, &reference, &entity, &BTreeMap::new(), None)
        .expect("lower tier fits policy")
        .expect("zero is a valid lower tier");
    let PmiDefinition::GeometricTolerance { magnitude, .. } = projected.definition else {
        panic!("lower profile tier definition");
    };
    assert_eq!(magnitude.get(), length(0.0).expect("finite zero length"));

    entity.doubles.insert("ToleranceLowerTier".into(), -1.0);
    assert!(
        project_lower_profile_tier(&ctx, &reference, &entity, &BTreeMap::new(), None)
            .expect("invalid lower tier fits policy")
            .is_none()
    );
    entity
        .doubles
        .insert("ToleranceLowerTier".into(), f64::INFINITY);
    assert!(
        project_lower_profile_tier(&ctx, &reference, &entity, &BTreeMap::new(), None)
            .expect("invalid lower tier fits policy")
            .is_none()
    );
}

fn put_pstr(bytes: &mut Vec<u8>, value: &str) {
    bytes.push(u8::try_from(value.len()).expect("fixture Pascal string"));
    bytes.extend_from_slice(value.as_bytes());
}

fn encode_entity(entity: &Entity, bytes: &mut Vec<u8>) {
    put_pstr(bytes, "Entity");
    put_pstr(bytes, &entity.class);
    put_pstr(bytes, "gdtanalysis.net");
    bytes.extend_from_slice(&1u32.to_le_bytes());
    if !entity.strings.is_empty() {
        put_pstr(bytes, "Strings");
        bytes.extend_from_slice(
            &u32::try_from(entity.strings.len())
                .expect("length fits u32")
                .to_le_bytes(),
        );
        for (key, value) in &entity.strings {
            put_pstr(bytes, key);
            put_pstr(bytes, value);
        }
        put_pstr(bytes, "EndStrings");
    }
    if !entity.integers.is_empty() {
        put_pstr(bytes, "Integers");
        bytes.extend_from_slice(
            &u32::try_from(entity.integers.len())
                .expect("length fits u32")
                .to_le_bytes(),
        );
        for (key, value) in &entity.integers {
            put_pstr(bytes, key);
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        put_pstr(bytes, "EndIntegers");
    }
    if !entity.doubles.is_empty() {
        put_pstr(bytes, "Doubles");
        bytes.extend_from_slice(
            &u32::try_from(entity.doubles.len())
                .expect("length fits u32")
                .to_le_bytes(),
        );
        for (key, value) in &entity.doubles {
            put_pstr(bytes, key);
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        put_pstr(bytes, "EndDoubles");
    }
    encode_objects("Features", "EndFeatures", &entity.features, bytes);
    encode_objects("Annotations", "EndAnnotations", &entity.annotations, bytes);
    if !entity.related.is_empty() {
        put_pstr(bytes, "RelatedObjects");
        bytes.extend_from_slice(
            &u32::try_from(entity.related.len())
                .expect("length fits u32")
                .to_le_bytes(),
        );
        for object in &entity.related {
            put_pstr(bytes, &object.name);
            put_pstr(bytes, &object.class);
        }
        for object in &entity.related {
            encode_entity(&object.entity, bytes);
        }
        put_pstr(bytes, "EndRelatedObjects");
    }
    put_pstr(bytes, "EndEntity");
}

fn encode_objects(name: &str, end: &str, section: &ObjectSection, bytes: &mut Vec<u8>) {
    if section.references.is_empty() && section.entities.is_empty() {
        return;
    }
    put_pstr(bytes, name);
    bytes.extend_from_slice(
        &u32::try_from(section.references.as_slice().len())
            .expect("length fits u32")
            .to_le_bytes(),
    );
    for reference in &section.references {
        put_pstr(bytes, &reference.id);
        put_pstr(bytes, &reference.class);
    }
    for entity in &section.entities {
        encode_entity(entity, bytes);
    }
    put_pstr(bytes, end);
}

fn encoded_root() -> Vec<u8> {
    let mut bytes = vec![0x11, 0x22, 0x33];
    encode_entity(&semantic_root(), &mut bytes);
    bytes
}

fn parse_root_with_service(payload: &[u8]) -> Option<Entity> {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &DecodePolicy::service())
        .expect("payload fits service policy");
    parse_unique_root(&ctx, payload).expect("SWIFT root fits service budget")
}

#[test]
fn swift_annotations_refuse_retained_stream_limit() {
    let root = Entity {
        class: crate::swift::ROOT_CLASS.into(),
        ..Entity::default()
    };
    let mut payload = Vec::new();
    encode_entity(&root, &mut payload);
    let mut source = crate::test_support::container::synthetic_sldprt();
    source.extend(crate::test_support::container::make_block(
        0x40,
        "SWIFT/Schema",
        &payload,
    ));
    let scan = crate::test_support::container::scan(&source);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(root.class.len()).expect("fixture length");
    let (ctx, _) =
        DecodeContext::from_root_bytes(&source, &arena, &policy).expect("source fits policy");
    let mut annotations = cadmpeg_ir::annotations::Annotations::default();
    let Err(CodecError::ResourceLimit(limit)) =
        crate::swift::annotations(&ctx, &scan, &mut annotations, None, None)
    else {
        panic!("expected retained refusal")
    };
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
}

fn swift_rendered_annotation_limit_error(
    set_limit: impl FnOnce(&mut cadmpeg_core::decode::ResourceLimits),
) -> CodecError {
    let mut payload = encoded_root();
    let literal = "<MOD-DIAM> .156";
    payload.extend_from_slice(&[0xff, 0xfe, 0xff]);
    payload.push(u8::try_from(literal.len()).expect("fixture length"));
    for unit in literal.encode_utf16() {
        payload.extend_from_slice(&unit.to_le_bytes());
    }
    let mut source = crate::test_support::container::synthetic_sldprt();
    source.extend(crate::test_support::container::make_block(
        0x40,
        "SWIFT/Schema",
        &payload,
    ));
    let scan = crate::test_support::container::scan(&source);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    set_limit(&mut policy.limits);
    let (ctx, _) =
        DecodeContext::from_root_bytes(&source, &arena, &policy).expect("source fits policy");
    let mut annotations = cadmpeg_ir::annotations::Annotations::default();
    crate::swift::annotations(&ctx, &scan, &mut annotations, None, None)
        .expect_err("SWIFT annotations must refuse")
}

#[test]
fn swift_annotations_refuse_work_limit() {
    let CodecError::ResourceLimit(limit) =
        swift_rendered_annotation_limit_error(|limits| limits.max_work_units = 0)
    else {
        panic!("expected work refusal")
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
}

#[test]
fn swift_annotations_refuse_scoped_limit() {
    let CodecError::ResourceLimit(limit) =
        swift_rendered_annotation_limit_error(|limits| limits.max_materialized_bytes = 0)
    else {
        panic!("expected scoped refusal")
    };
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
}

#[test]
fn swift_annotations_refuse_collection_limit() {
    let CodecError::ResourceLimit(limit) =
        swift_rendered_annotation_limit_error(|limits| limits.max_collection_items = 0)
    else {
        panic!("expected collection refusal")
    };
    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
}

#[test]
fn swift_annotations_refuse_nesting_limit() {
    let CodecError::ResourceLimit(limit) =
        swift_rendered_annotation_limit_error(|limits| limits.max_recursion_depth = 0)
    else {
        panic!("expected nesting refusal")
    };
    assert_eq!(limit.dimension, ResourceDimension::RecursionDepth);
}

#[test]
fn parses_and_projects_semantic_graph() {
    let parsed = parse_root_with_service(&encoded_root()).expect("synthetic SWIFT root");
    let annotations = project(&parsed);
    assert_eq!(
        annotations
            .iter()
            .map(|annotation| annotation.id.clone())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([
            pmi_id("A10").unwrap(),
            pmi_id("A20").unwrap(),
            pmi_id("A20:datum-system").unwrap(),
            pmi_id("A30").unwrap(),
            pmi_id("A40").unwrap(),
        ])
    );
    assert_eq!(dimension_nominal(&annotations, "A30"), None);

    let position = annotations
        .iter()
        .find(|annotation| annotation.name.as_deref() == Some("Position 1"))
        .expect("position annotation");
    let PmiDefinition::GeometricTolerance {
        magnitude,
        datum_system,
        modifiers,
        ..
    } = &position.definition
    else {
        panic!("position definition");
    };
    assert_eq!(magnitude.get(), length(0.25).expect("finite length"));
    assert_eq!(
        datum_system.as_ref().map(cadmpeg_ir::ids::PmiId::as_str),
        Some("sldprt:model:pmi#A20:datum-system")
    );
    assert_eq!(
        modifiers,
        &["maximum_material_requirement", "projected_zone:4_mm"]
    );
    assert_eq!(
        position.targets,
        [
            PmiTarget::ShapeAspect {
                source_id: cadmpeg_core::text::NonBlankString::new("F20")
                    .expect("nonempty source identity")
            },
            PmiTarget::ShapeAspect {
                source_id: cadmpeg_core::text::NonBlankString::new("F21")
                    .expect("nonempty source identity")
            }
        ]
    );

    let system = annotations
        .iter()
        .find(|annotation| annotation.id.as_str().ends_with(":datum-system"))
        .expect("datum system");
    let PmiDefinition::DatumSystem { references } = &system.definition else {
        panic!("datum-system definition");
    };
    assert_eq!(references.as_slice().len(), 1);
    let datum_reference = references
        .as_slice()
        .first()
        .expect("primary datum reference");
    assert_eq!(datum_reference.precedence.get(), 1);
    assert_eq!(datum_reference.modifiers, ["least_material_requirement"]);

    assert_eq!(dimension_nominal(&annotations, "A30"), None);

    let angle = annotations
        .iter()
        .find(|annotation| annotation.id.as_str().ends_with("#A40"))
        .expect("angular annotation");
    let PmiDefinition::Dimension(relation) = &angle.definition else {
        panic!("angular definition");
    };
    let nominal = relation.nominal().expect("angular nominal");
    assert_eq!(
        *nominal,
        PmiValue::new(0.0, PmiQuantity::Angle).expect("finite angle")
    );
}

#[test]
fn repeated_swift_tolerances_share_one_datum_system() {
    let mut root = semantic_root();
    let second_position = root
        .annotations
        .entities
        .get(1)
        .expect("position annotation")
        .clone();
    root.annotations
        .references
        .push(super::reference("A21", "GdtPosition"));
    root.annotations.entities.push(second_position);
    let projected = project(&root);
    let systems = projected
        .iter()
        .filter(|annotation| matches!(&annotation.definition, PmiDefinition::DatumSystem { .. }))
        .count();
    assert_eq!(systems, 1);
    for id in ["A20", "A21"] {
        let annotation = projected
            .iter()
            .find(|annotation| annotation.id == pmi_id(id).expect("valid PMI ID"))
            .expect("position tolerance");
        let PmiDefinition::GeometricTolerance { datum_system, .. } = &annotation.definition else {
            panic!("position tolerance definition")
        };
        assert_eq!(
            datum_system.as_ref().map(cadmpeg_ir::ids::PmiId::as_str),
            Some("sldprt:model:pmi#A20:datum-system")
        );
    }
}

#[test]
fn rejects_ambiguous_root_and_impossible_count() {
    let encoded = encoded_root();
    let mut duplicate = encoded.clone();
    duplicate.extend_from_slice(&encoded);
    assert_eq!(parse_root_with_service(&duplicate), None);

    let mut malformed = encoded;
    let marker = b"\x0bAnnotations";
    let offset = malformed
        .windows(marker.len())
        .position(|window| window == marker)
        .expect("root annotations marker")
        + marker.len();
    malformed
        .get_mut(offset..offset + 4)
        .expect("count field")
        .copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(parse_root_with_service(&malformed), None);
}

#[test]
fn malformed_reference_identity_returns_decode_loss() {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    for id in ["", "A 10"] {
        let mut root = semantic_root();
        root.annotations.references.first_mut().unwrap().id = id.into();
        let mut payload = Vec::new();
        encode_entity(&root, &mut payload);
        let parsed = parse_root_with_service(&payload).unwrap();
        assert_eq!(parsed.annotations.references.first().unwrap().id, id);
        let mut bytes = crate::test_support::container::synthetic_sldprt();
        bytes.extend(crate::test_support::container::make_block(
            0x40,
            "SWIFT/Schema",
            &payload,
        ));
        let decoded = crate::SldprtCodec
            .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        assert!(decoded
            .report()
            .losses
            .iter()
            .any(|loss| loss.code
                == crate::loss::SldprtLossCode::PmiSwiftAnnotationUnsupported.kind()));
        assert!(!project(&parsed)
            .iter()
            .any(|annotation| annotation.name.as_deref() == Some("Datum A")));
    }
}

#[test]
fn swift_serialized_entity_depth_refuses_instead_of_unresolved_root() {
    let mut nested = Entity {
        class: "Leaf".into(),
        ..Default::default()
    };
    for _ in 0..crate::swift::MAX_DEPTH {
        nested = Entity {
            class: crate::swift::ROOT_CLASS.into(),
            related: vec![crate::swift::RelatedObject {
                name: "Child".into(),
                class: nested.class.clone(),
                entity: nested,
            }],
            ..Default::default()
        };
    }
    let mut payload = Vec::new();
    encode_entity(&nested, &mut payload);
    let ctx = cadmpeg_test_support::service_decode_context();
    let error = parse_unique_root(&ctx, &payload).unwrap_err();
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.operation == "parse SWIFT entity")
    );
}

#[test]
fn duplicate_swift_annotation_reference_ids_refuse_before_projection() {
    let mut annotation = super::entity("GdtFlatness");
    annotation.doubles.insert("Tolerance".into(), 0.25);
    let mut root = super::entity("GdtPart");
    root.class = crate::swift::ROOT_CLASS.into();
    root.annotations.references = vec![super::reference("A42", "GdtFlatness"); 2];
    root.annotations.entities = vec![annotation.clone(), annotation];
    let mut payload = Vec::new();
    encode_entity(&root, &mut payload);
    let ctx = cadmpeg_test_support::service_decode_context();
    let error = parse_unique_root(&ctx, &payload).unwrap_err();
    assert!(matches!(&error, CodecError::Malformed(message) if message == "duplicate SWIFT object reference ID A42"));
    let mut source = crate::test_support::container::synthetic_sldprt();
    source.extend(crate::test_support::container::make_block(0x40, "SWIFT/Schema", &payload));
    use cadmpeg_ir::Codec as _;
    let error = crate::SldprtCodec.decode(&mut std::io::Cursor::new(source), &cadmpeg_ir::DecodeOptions::default()).unwrap_err();
    assert!(matches!(&error, cadmpeg_ir::DecodeFailure::Codec(CodecError::Malformed(message)) if message == "duplicate SWIFT object reference ID A42"));
}

#[test]
fn swift_roster_constructor_rejects_duplicate_ids_and_wrong_bindings() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let reference = super::reference("A42", "GdtFlatness");
    let entity = super::entity("GdtFlatness");
    assert!(matches!(ObjectSection::new(&ctx, vec![reference.clone(), reference.clone()], vec![entity.clone(), entity.clone()]), Err(CodecError::Malformed(_))));
    assert!(matches!(ObjectSection::new(&ctx, vec![reference.clone()], vec![super::entity("GdtDatum")]), Ok(None)));
    assert!(matches!(ObjectSection::new(&ctx, vec![], vec![entity.clone()]), Ok(None)));
    assert!(ObjectSection::new(&ctx, vec![reference], vec![entity]).unwrap().is_some());
    assert!(ObjectSection::new(&ctx, vec![], vec![]).unwrap().is_some());
}

#[test]
fn swift_roster_identity_admission_preserves_resource_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = ObjectSection::new(&ctx, vec![super::reference("A42", "GdtFlatness")], vec![super::entity("GdtFlatness")]).unwrap_err();
    let CodecError::ResourceLimit(limit) = error else { panic!("identity admission refusal"); };
    assert_eq!(limit.operation, "admit distinct SWIFT object references");
    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
    assert_eq!(ctx.resource_refusal(), Some(limit));
}

fn source_with_swift_annotation(annotation: Entity) -> Vec<u8> {
    let mut root = Entity { class: crate::swift::ROOT_CLASS.into(), ..Entity::default() };
    root.annotations.references.push(Reference {
        id: "A42".into(), class: annotation.class.clone(),
    });
    root.annotations.entities.push(annotation);
    let mut payload = Vec::new();
    encode_entity(&root, &mut payload);
    let mut source = crate::test_support::container::outer_header();
    source.extend(crate::test_support::container::make_block(0x40, "SWIFT/Schema", &payload));
    source
}

fn assert_malformed_swift_projection(annotation: Entity, reason: &str) {
    use cadmpeg_ir::codec::Codec;
    let class = crate::swift::short_class(&annotation.class).to_owned();
    let source = source_with_swift_annotation(annotation);
    let scan = crate::test_support::container::scan(&source);
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut provenance = cadmpeg_ir::annotations::Annotations::default();
    let expected = format!("SWIFT annotation A42 ({class}): {reason}");
    assert!(matches!(crate::swift::annotations(&ctx, &scan, &mut provenance, None, None),
        Err(CodecError::Malformed(message)) if message == expected));
    assert!(matches!(crate::swift::unsupported_annotation_classes(&ctx, &scan),
        Err(CodecError::Malformed(message)) if message == expected));
    assert!(matches!(crate::SldprtCodec.decode(&mut std::io::Cursor::new(source), &cadmpeg_ir::codec::DecodeOptions::default()),
        Err(cadmpeg_ir::codec::DecodeFailure::Codec(CodecError::Malformed(message))) if message == expected));
}

#[test]
fn swift_missing_tolerance_refuses_instead_of_silent_omission() {
    assert_malformed_swift_projection(super::entity("GdtFlatness"), "Tolerance must be present, finite and non-negative");
}

#[test]
fn swift_negative_tolerance_refuses_instead_of_silent_omission() {
    let mut annotation = super::entity("GdtFlatness");
    annotation.doubles.insert("Tolerance".into(), -1.0);
    assert_malformed_swift_projection(annotation, "Tolerance must be present, finite and non-negative");
}

#[test]
fn swift_nonfinite_tolerance_refuses_instead_of_silent_omission() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut annotation = super::entity("GdtFlatness");
        annotation.doubles.insert("Tolerance".into(), value);
        assert_malformed_swift_projection(annotation, "Tolerance must be present, finite and non-negative");
    }
}

#[test]
fn swift_empty_datum_identifier_refuses_instead_of_silent_omission() {
    let mut annotation = super::entity("GdtDatum");
    annotation.strings.insert("DatumIdentifier".into(), String::new());
    assert_malformed_swift_projection(annotation, "missing or empty DatumIdentifier");
}

#[test]
fn swift_missing_datum_identifier_refuses_instead_of_silent_omission() {
    assert_malformed_swift_projection(super::entity("GdtDatum"), "missing or empty DatumIdentifier");
}

#[test]
fn swift_valid_and_suppressed_annotations_have_explicit_dispositions() {
    for value in [0.0, 0.25] {
        let mut annotation = super::entity("GdtFlatness");
        annotation.doubles.insert("Tolerance".into(), value);
        let source = source_with_swift_annotation(annotation);
        let scan = crate::test_support::container::scan(&source);
        let ctx = cadmpeg_test_support::service_decode_context();
        let mut provenance = cadmpeg_ir::annotations::Annotations::default();
        let projected = crate::swift::annotations(&ctx, &scan, &mut provenance, None, None).unwrap();
        assert_eq!(projected.len(), 1);
        assert_eq!(projected[0].id, pmi_id("A42").unwrap());
        let PmiDefinition::GeometricTolerance { magnitude, .. } = projected[0].definition else {
            panic!("expected flatness tolerance");
        };
        assert_eq!(magnitude.get(), length(value).unwrap());
        assert!(crate::swift::unsupported_annotation_classes(&ctx, &scan).unwrap().is_empty());
    }
    for class in ["GdtFlatness", "GdtDatum", "GdtUnsupported"] {
        let mut annotation = super::entity(class);
        annotation.integers.insert("IsSuppressed".into(), 1);
        let source = source_with_swift_annotation(annotation);
        let scan = crate::test_support::container::scan(&source);
        let ctx = cadmpeg_test_support::service_decode_context();
        let mut provenance = cadmpeg_ir::annotations::Annotations::default();
        assert!(crate::swift::annotations(&ctx, &scan, &mut provenance, None, None).unwrap().is_empty());
        assert!(crate::swift::unsupported_annotation_classes(&ctx, &scan).unwrap().is_empty());
    }
}

#[test]
fn swift_recognized_unprojected_annotation_records_unsupported_loss() {
    use cadmpeg_ir::codec::Codec;
    let source = source_with_swift_annotation(super::entity("GdtWidth"));
    let scan = crate::test_support::container::scan(&source);
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut provenance = cadmpeg_ir::annotations::Annotations::default();
    assert!(crate::swift::annotations(&ctx, &scan, &mut provenance, None, None).unwrap().is_empty());
    assert_eq!(crate::swift::unsupported_annotation_classes(&ctx, &scan).unwrap(), BTreeMap::from([("GdtWidth".into(), 1)]));
    let result = crate::SldprtCodec.decode(&mut std::io::Cursor::new(source), &cadmpeg_ir::codec::DecodeOptions::default()).unwrap();
    assert!(result.report().losses.iter().any(|loss| loss.message.contains("GdtWidth (1)")));
}

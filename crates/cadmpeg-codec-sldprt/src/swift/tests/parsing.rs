use super::dimension_nominal;
use super::length;
use super::semantic_root;
use crate::swift::annotations;
use crate::swift::parse_unique_root;
use crate::swift::pmi_id;
use crate::swift::project;
use crate::swift::project_lower_profile_tier;
use crate::swift::Entity;
use crate::swift::ObjectSection;
use crate::swift::Reference;
use crate::swift::RelatedObject;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::Codec;
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
    let projected = project_lower_profile_tier(
        &ctx,
        &reference,
        &entity,
        &BTreeMap::new(),
        &crate::swift::ProjectionGeometry::new(&ctx, None, None, None),
    )
    .expect("lower tier fits policy")
    .expect("zero is a valid lower tier");
    let PmiDefinition::GeometricTolerance { magnitude, .. } = projected.definition else {
        panic!("lower profile tier definition");
    };
    assert_eq!(magnitude.get(), length(0.0).expect("finite zero length"));

    entity.doubles.insert("ToleranceLowerTier".into(), -1.0);
    assert!(project_lower_profile_tier(
        &ctx,
        &reference,
        &entity,
        &BTreeMap::new(),
        &crate::swift::ProjectionGeometry::new(&ctx, None, None, None)
    )
    .expect("invalid lower tier fits policy")
    .is_none());
    entity
        .doubles
        .insert("ToleranceLowerTier".into(), f64::INFINITY);
    assert!(project_lower_profile_tier(
        &ctx,
        &reference,
        &entity,
        &BTreeMap::new(),
        &crate::swift::ProjectionGeometry::new(&ctx, None, None, None)
    )
    .expect("invalid lower tier fits policy")
    .is_none());
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
    crate::swift::annotations(&ctx, &scan, &mut annotations, None)
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
                source_id: cadmpeg_core::text::NonBlankString::try_from("F20")
                    .expect("nonempty source identity")
            },
            PmiTarget::ShapeAspect {
                source_id: cadmpeg_core::text::NonBlankString::try_from("F21")
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
    assert!(
        matches!(&error, CodecError::Malformed(message) if message == "duplicate SWIFT object reference ID A42")
    );
    let mut source = crate::test_support::container::synthetic_sldprt();
    source.extend(crate::test_support::container::make_block(
        0x40,
        "SWIFT/Schema",
        &payload,
    ));
    let error = crate::SldprtCodec
        .decode(
            &mut std::io::Cursor::new(source),
            &cadmpeg_ir::DecodeOptions::default(),
        )
        .unwrap_err();
    assert!(
        matches!(&error, cadmpeg_ir::DecodeFailure::Codec(CodecError::Malformed(message)) if message == "duplicate SWIFT object reference ID A42")
    );
}

#[test]
fn swift_roster_constructor_rejects_duplicate_ids_and_wrong_bindings() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let reference = super::reference("A42", "GdtFlatness");
    let entity = super::entity("GdtFlatness");
    assert!(matches!(
        ObjectSection::new(
            &ctx,
            vec![reference.clone(), reference.clone()],
            vec![entity.clone(), entity.clone()]
        ),
        Err(CodecError::Malformed(_))
    ));
    assert!(matches!(
        ObjectSection::new(
            &ctx,
            vec![reference.clone()],
            vec![super::entity("GdtDatum")]
        ),
        Ok(None)
    ));
    assert!(matches!(
        ObjectSection::new(&ctx, vec![], vec![entity.clone()]),
        Ok(None)
    ));
    assert!(ObjectSection::new(&ctx, vec![reference], vec![entity])
        .unwrap()
        .is_some());
    assert!(ObjectSection::new(&ctx, vec![], vec![]).unwrap().is_some());
}

#[test]
fn swift_roster_identity_admission_preserves_resource_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = ObjectSection::new(
        &ctx,
        vec![super::reference("A42", "GdtFlatness")],
        vec![super::entity("GdtFlatness")],
    )
    .unwrap_err();
    let CodecError::ResourceLimit(limit) = error else {
        panic!("identity admission refusal");
    };
    assert_eq!(limit.operation, "admit distinct SWIFT object references");
    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
    assert_eq!(ctx.resource_refusal(), Some(limit));
}

fn source_with_swift_root(root: &Entity) -> Vec<u8> {
    let mut payload = Vec::new();
    encode_entity(root, &mut payload);
    let mut source = crate::test_support::container::outer_header();
    source.extend(crate::test_support::container::make_block(
        0x40,
        "SWIFT/Schema",
        &payload,
    ));
    source
}

fn source_with_swift_annotation(annotation: Entity) -> Vec<u8> {
    let mut root = Entity {
        class: crate::swift::ROOT_CLASS.into(),
        ..Entity::default()
    };
    root.annotations.references.push(Reference {
        id: "A42".into(),
        class: annotation.class.clone(),
    });
    root.annotations.entities.push(annotation);
    source_with_swift_root(&root)
}

fn assert_malformed_swift_projection(annotation: Entity, reason: &str) {
    let class = crate::swift::short_class(
        &cadmpeg_test_support::service_decode_context(),
        &annotation.class,
    )
    .unwrap()
    .to_owned();
    let source = source_with_swift_annotation(annotation);
    let scan = crate::test_support::container::scan(&source);
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut provenance = cadmpeg_ir::annotations::Annotations::default();
    let expected = format!("SWIFT annotation A42 ({class}): {reason}");
    assert!(matches!(
        crate::swift::annotations(&ctx, &scan, &mut provenance, None),
        Err(CodecError::Malformed(message)) if message == expected
    ));
    assert!(
        matches!(crate::SldprtCodec.decode(&mut std::io::Cursor::new(source), &cadmpeg_ir::codec::DecodeOptions::default()),
        Err(cadmpeg_ir::codec::DecodeFailure::Codec(CodecError::Malformed(message))) if message == expected)
    );
}

#[test]
fn swift_missing_tolerance_refuses_instead_of_silent_omission() {
    assert_malformed_swift_projection(
        super::entity("GdtFlatness"),
        "Tolerance must be present, finite and non-negative",
    );
}

#[test]
fn swift_negative_tolerance_refuses_instead_of_silent_omission() {
    let mut annotation = super::entity("GdtFlatness");
    annotation.doubles.insert("Tolerance".into(), -1.0);
    assert_malformed_swift_projection(
        annotation,
        "Tolerance must be present, finite and non-negative",
    );
}

#[test]
fn swift_nonfinite_tolerance_refuses_instead_of_silent_omission() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut annotation = super::entity("GdtFlatness");
        annotation.doubles.insert("Tolerance".into(), value);
        assert_malformed_swift_projection(
            annotation,
            "Tolerance must be present, finite and non-negative",
        );
    }
}

#[test]
fn swift_empty_datum_identifier_refuses_instead_of_silent_omission() {
    let mut annotation = super::entity("GdtDatum");
    annotation
        .strings
        .insert("DatumIdentifier".into(), String::new());
    assert_malformed_swift_projection(annotation, "missing or empty DatumIdentifier");
}

#[test]
fn swift_missing_datum_identifier_refuses_instead_of_silent_omission() {
    assert_malformed_swift_projection(
        super::entity("GdtDatum"),
        "missing or empty DatumIdentifier",
    );
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
        let projection = annotations(&ctx, &scan, &mut provenance, None).unwrap();
        assert_eq!(projection.annotations.len(), 1);
        let annotation = projection.annotations.first().unwrap();
        assert_eq!(annotation.id, pmi_id("A42").unwrap());
        let PmiDefinition::GeometricTolerance { magnitude, .. } = &annotation.definition else {
            panic!("expected flatness tolerance");
        };
        assert_eq!(magnitude.get(), length(value).unwrap());
        assert!(projection.unsupported_loss.is_none());
    }
    for class in ["GdtFlatness", "GdtDatum", "GdtUnsupported"] {
        let mut annotation = super::entity(class);
        annotation.integers.insert("IsSuppressed".into(), 1);
        let source = source_with_swift_annotation(annotation);
        let scan = crate::test_support::container::scan(&source);
        let ctx = cadmpeg_test_support::service_decode_context();
        let mut provenance = cadmpeg_ir::annotations::Annotations::default();
        let projection = annotations(&ctx, &scan, &mut provenance, None).unwrap();
        assert!(projection.annotations.is_empty());
        assert!(projection.unsupported_loss.is_none());
    }
}

#[test]
fn swift_recognized_unprojected_annotation_records_unsupported_loss() {
    let source = source_with_swift_annotation(super::entity("GdtWidth"));
    let scan = crate::test_support::container::scan(&source);
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut provenance = cadmpeg_ir::annotations::Annotations::default();
    let projection = annotations(&ctx, &scan, &mut provenance, None)
        .expect("unsupported annotation is still parsed once");
    assert!(projection.annotations.is_empty());
    assert_eq!(
        projection.unsupported_loss.as_deref(),
        Some("1 SWIFT semantic annotation(s) have no neutral PMI definition: GdtWidth (1).")
    );
    let result = crate::SldprtCodec
        .decode(
            &mut std::io::Cursor::new(source),
            &cadmpeg_ir::codec::DecodeOptions::default(),
        )
        .unwrap();
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("GdtWidth (1)")));
}

#[test]
fn unsupported_swift_loss_refuses_collection_limit() {
    let source = source_with_swift_annotation(super::entity("GdtWidth"));
    let scan = crate::test_support::container::scan(&source);
    let arena = DecodeArena::new();
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::CollectionItems,
        "collect SLDPRT unsupported SWIFT classes",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&source, &arena, &policy)?;
            let mut provenance = cadmpeg_ir::annotations::Annotations::default();
            crate::swift::annotations(&ctx, &scan, &mut provenance, None).map(|_| ())
        },
    );
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "collect SLDPRT unsupported SWIFT classes"
    ));
}

#[test]
fn unsupported_swift_loss_refuses_scoped_limit() {
    let unsupported = BTreeMap::from([("GdtWidth".into(), 1)]);
    let arena = DecodeArena::new();
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::MaterializedBytes,
        "format SLDPRT unsupported SWIFT classes",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
            crate::swift::format_unsupported_loss(&ctx, &unsupported).map(drop)
        },
    );
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::MaterializedBytes
                && limit.operation == "format SLDPRT unsupported SWIFT classes"
    ));
}

fn assert_geometry_indexes_skipped(root: &Entity, expected_loss: Option<&str>) {
    use cadmpeg_core::decode::{
        refusal_probe::RefusalProbe, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension,
    };
    use cadmpeg_ir::ids::BodyId;
    use cadmpeg_ir::topology::{Body, BodyKind};

    let source = source_with_swift_root(root);
    let scan = crate::test_support::container::scan(&source);
    let body = Body {
        id: BodyId::mint("sldprt:brep:body#11").expect("identity grammar"),
        kind: BodyKind::default(),
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    };
    let features = super::identity::pattern_hole_features();
    assert!(!features.is_empty());

    for operation in [
        "index SWIFT body identities",
        "swift pattern hole candidates",
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        let (ctx, _) = DecodeContext::from_root_bytes(&source, &arena, &policy)
            .expect("SWIFT root fits policy");
        let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
        let mut provenance = cadmpeg_ir::annotations::Annotations::default();
        let projection = annotations(
            &ctx,
            &scan,
            &mut provenance,
            Some(crate::swift::GeometryProjectionInputs {
                topology: crate::swift::PrimaryTopology {
                    bodies: std::slice::from_ref(&body),
                    faces: &[],
                    edges: &[],
                    vertices: &[],
                },
                face_bridge_sequences: &[],
                edge_use_sequences: &[],
                vertex_use_sequences: &[],
                features: &features,
            }),
        )
        .expect("empty or incomplete annotation roster skips geometry indexes");
        assert!(projection.annotations.is_empty());
        assert_eq!(projection.unsupported_loss.as_deref(), expected_loss);
        ctx.finish_session().expect("accepted session");
    }
}

#[test]
fn swift_empty_annotation_roster_skips_geometry_indexes() {
    let root = Entity {
        class: crate::swift::ROOT_CLASS.into(),
        ..Entity::default()
    };
    assert_geometry_indexes_skipped(&root, None);
}

#[test]
fn swift_incomplete_annotation_roster_keeps_loss_without_geometry_indexes() {
    let mut root = Entity {
        class: crate::swift::ROOT_CLASS.into(),
        ..Entity::default()
    };
    root.annotations.references.push(Reference {
        id: "A42".into(),
        class: "GdtWidth".into(),
    });
    assert_geometry_indexes_skipped(
        &root,
        Some("1 SWIFT semantic annotation(s) have no neutral PMI definition: GdtAnalysisIncompleteAnnotationRoster (1)."),
    );
}

#[test]
fn swift_pattern_diameter_report_uses_the_projection_nominal_context() {
    use super::identity::{cad_feature, pattern_hole_features};

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("empty root fits policy");
    let features = pattern_hole_features();

    let mut pattern = cad_feature("GdtPattern", "");
    pattern
        .strings
        .insert("ObjectName".into(), "Hole Pattern6".into());
    pattern.related.push(RelatedObject {
        name: "SubFeatures".into(),
        class: "PrizMetrik.GdtAnalysis.GdtAppliedFeatureCollection".into(),
        entity: super::entity("GdtAppliedFeatureCollection"),
    });
    let mut diameter = super::entity("GdtDiameter");
    diameter
        .strings
        .insert("ObjectName".into(), "Diameter 8".into());
    diameter.doubles.insert("Nominal".into(), 0.0);
    diameter
        .features
        .references
        .push(super::reference("FP", "GdtPattern"));
    let root = Entity {
        class: crate::swift::ROOT_CLASS.into(),
        features: ObjectSection {
            references: vec![super::reference("FP", "GdtPattern")],
            entities: vec![pattern],
        },
        annotations: ObjectSection {
            references: vec![super::reference("A8", "GdtDiameter")],
            entities: vec![diameter],
        },
        ..Entity::default()
    };
    let mut payload = Vec::new();
    encode_entity(&root, &mut payload);
    let mut source = crate::test_support::container::outer_header();
    source.extend(crate::test_support::container::make_block(
        0x40,
        "SWIFT/Schema",
        &payload,
    ));
    let scan = crate::test_support::container::scan(&source);
    let mut provenance = cadmpeg_ir::annotations::Annotations::default();
    let projected = annotations(
        &ctx,
        &scan,
        &mut provenance,
        Some(crate::swift::GeometryProjectionInputs {
            topology: crate::swift::PrimaryTopology {
                bodies: &[],
                faces: &[],
                edges: &[],
                vertices: &[],
            },
            face_bridge_sequences: &[],
            edge_use_sequences: &[],
            vertex_use_sequences: &[],
            features: &features,
        }),
    )
    .expect("same projection supplies annotation and loss classification");

    assert_eq!(
        dimension_nominal(&projected.annotations, "A8"),
        length(6.1468),
        "the zero source nominal is filled from the unique native pattern seed"
    );
    assert!(projected.unsupported_loss.is_none());
}

#[test]
fn swift_provenance_index_preserves_overlapping_source_prefixes() {
    let mut root = Entity {
        class: crate::swift::ROOT_CLASS.into(),
        ..Entity::default()
    };
    for id in ["A:child", "A", "Aother"] {
        let mut annotation = super::entity("GdtFlatness");
        annotation.doubles.insert("Tolerance".into(), 0.25);
        root.annotations.references.push(Reference {
            id: id.into(),
            class: annotation.class.clone(),
        });
        root.annotations.entities.push(annotation);
    }
    let mut payload = Vec::new();
    encode_entity(&root, &mut payload);
    let parsed = parse_root_with_service(&payload).unwrap();
    let mut source = crate::test_support::container::outer_header();
    source.extend(crate::test_support::container::make_block(
        0x40,
        "SWIFT/Schema",
        &payload,
    ));
    let scan = crate::test_support::container::scan(&source);
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut provenance = cadmpeg_ir::annotations::Annotations::default();
    let projected = crate::swift::annotations(&ctx, &scan, &mut provenance, None).unwrap();
    assert_eq!(
        projected
            .annotations
            .iter()
            .map(|annotation| annotation.id.as_str())
            .collect::<Vec<_>>(),
        [
            "sldprt:model:pmi#A:child",
            "sldprt:model:pmi#A",
            "sldprt:model:pmi#Aother"
        ]
    );
    for (id, index) in [("A:child", 1), ("A", 1), ("Aother", 2)] {
        assert_eq!(
            provenance
                .provenance
                .get(pmi_id(id).unwrap().as_str())
                .unwrap()
                .offset,
            cadmpeg_core::decode::u64_from_index(
                parsed.annotations.entities.get(index).unwrap().offset
            )
        );
    }
}

#[test]
fn swift_class_suffix_search_visits_only_the_suffix() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::try_from(".GdtCylinder".len()).unwrap();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let class = format!("{}.GdtCylinder", "長namespace".repeat(1024));
    assert_eq!(
        crate::swift::short_class(&ctx, &class).unwrap(),
        "GdtCylinder"
    );
    ctx.finish_session().unwrap();
}

#[test]
fn swift_object_prefix_validation_does_not_precharge_unused_pairs() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let references: Vec<_> = (0..128)
        .map(|index| Reference {
            id: format!("F{index}"),
            class: "First".into(),
        })
        .collect();
    let mismatched = Entity {
        class: "Other".into(),
        ..Entity::default()
    };
    let arena = DecodeArena::new();
    let run = |cap| {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
        ObjectSection::new(&ctx, references.clone(), vec![mismatched.clone()])
    };
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "scan SWIFT object pairs",
        run,
    );
    let CodecError::ResourceLimit(limit) = error else {
        panic!("pair boundary");
    };
    assert_eq!(limit.additional, 1);
    // One pair step, the comma-search bytes and end probe, and both comparison operands.
    let search = u64::try_from("First".len()).unwrap() + 1;
    let comparison = u64::try_from("First".len() + "Other".len()).unwrap();
    assert!(run(limit.used + 1 + search + comparison).unwrap().is_none());
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = limit.used + 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(ObjectSection::new(&ctx, references, Vec::new())
        .unwrap()
        .is_some());
    ctx.finish_session().unwrap();
}

#[test]
fn swift_reference_class_search_stops_at_the_comma() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let reference = Reference {
        id: "F0".into(),
        class: format!("First,{}", "unused".repeat(1024)),
    };
    let entity = Entity {
        class: "First".into(),
        ..Entity::default()
    };
    let arena = DecodeArena::new();
    let run = |cap| {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
        ObjectSection::new(&ctx, vec![reference.clone()], vec![entity.clone()])
    };
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "bind SWIFT reference classes",
        run,
    );
    let CodecError::ResourceLimit(limit) = error else {
        panic!("class search boundary");
    };
    // The prefix bytes, both comparison operands, and the final pair probe.
    let search = u64::try_from("First,".len()).unwrap();
    let comparison = u64::try_from(2 * "First".len()).unwrap();
    assert!(run(limit.used + search + comparison + 1).unwrap().is_some());
}

#[test]
fn swift_empty_projection_skips_feature_and_provenance_work() {
    use cadmpeg_core::decode::refusal_probe::RefusalProbe;
    let mut root = Entity {
        class: crate::swift::ROOT_CLASS.into(),
        ..Entity::default()
    };
    root.features
        .references
        .push(super::reference("F10", "GdtPlane"));
    root.features.entities.push(super::entity("GdtPlane"));
    let source = source_with_swift_root(&root);
    let scan = crate::test_support::container::scan(&source);
    for operation in [
        "index SWIFT features",
        "index SWIFT annotation provenance",
        "scan SWIFT annotation references",
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        let (ctx, _) = DecodeContext::from_root_bytes(&source, &arena, &policy).unwrap();
        let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
        let projection = annotations(
            &ctx,
            &scan,
            &mut cadmpeg_ir::annotations::Annotations::default(),
            None,
        )
        .unwrap();
        assert!(projection.annotations.is_empty());
        assert!(projection.unsupported_loss.is_none());
        ctx.finish_session().unwrap();
    }
    root.annotations
        .references
        .push(super::reference("A10", "GdtFlatness"));
    let source = source_with_swift_root(&root);
    let scan = crate::test_support::container::scan(&source);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&source, &arena, &policy).unwrap();
    let _probe = RefusalProbe::arm(
        ResourceDimension::WorkUnits,
        "scan SWIFT annotation references",
        None,
    );
    let projection = annotations(
        &ctx,
        &scan,
        &mut cadmpeg_ir::annotations::Annotations::default(),
        None,
    )
    .unwrap();
    assert!(projection.annotations.is_empty());
    assert!(projection
        .unsupported_loss
        .unwrap()
        .contains("GdtAnalysisIncompleteAnnotationRoster"));
    ctx.finish_session().unwrap();
}

#[test]
fn swift_discarded_entity_candidates_release_their_storage() {
    let mut payload = Vec::new();
    for _ in 0..64 {
        encode_entity(&super::entity("GdtPlane"), &mut payload);
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 1024 * 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(parse_unique_root(&ctx, &payload).unwrap().is_none());
    let all_scratch = ctx
        .reserve_scoped(
            policy.limits.max_materialized_bytes,
            "reuse SWIFT discovery scratch",
        )
        .unwrap();
    drop(all_scratch);
    ctx.finish_session().unwrap();
}

#[test]
fn swift_root_discovery_checks_nested_and_property_root_tokens() {
    let nested_root = Entity {
        class: crate::swift::ROOT_CLASS.into(),
        ..Entity::default()
    };
    let mut root = nested_root.clone();
    root.related.push(RelatedObject {
        name: "Nested".into(),
        class: nested_root.class.clone(),
        entity: nested_root,
    });
    let mut payload = Vec::new();
    encode_entity(&root, &mut payload);
    let ctx = cadmpeg_test_support::service_decode_context();
    assert!(parse_unique_root(&ctx, &payload).unwrap().is_none());
    let root = Entity {
        class: crate::swift::ROOT_CLASS.into(),
        ..Entity::default()
    };
    let mut embedded = Vec::new();
    encode_entity(&root, &mut embedded);
    let embedded = String::from_utf8(embedded).unwrap();
    let mut root = root;
    root.strings.insert("Embedded".into(), embedded);
    let mut payload = Vec::new();
    encode_entity(&root, &mut payload);
    let ctx = cadmpeg_test_support::service_decode_context();
    assert!(parse_unique_root(&ctx, &payload).unwrap().is_none());
}

#[test]
fn swift_discovery_holds_only_the_selected_tree_storage() {
    let mut root = Entity {
        class: crate::swift::ROOT_CLASS.into(),
        ..Entity::default()
    };
    for index in 0..64 {
        root.features
            .references
            .push(super::reference(&format!("F{index}"), "GdtPlane"));
        root.features.entities.push(super::entity("GdtPlane"));
    }
    let mut payload = Vec::new();
    encode_entity(&root, &mut payload);
    let live_storage = |discover| {
        let arena = DecodeArena::new();
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::MaterializedBytes,
            "measure selected SWIFT tree",
            |cap| {
                let mut policy = DecodePolicy::service();
                policy.limits.max_materialized_bytes = cap;
                policy.limits.max_retained_bytes = 0;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
                let (parsed, storage) =
                    ctx.with_scoped_storage("selected SWIFT test tree", || {
                        if discover {
                            parse_unique_root(&ctx, &payload)
                        } else {
                            crate::swift::parse_entity(
                                &ctx,
                                &mut cadmpeg_core::decode::View::over_retained(&payload),
                                0,
                            )
                        }
                    })?;
                assert_eq!(parsed.as_ref().unwrap().features.entities.len(), 64);
                let measurement =
                    ctx.reserve_scoped(u64::MAX / 2, "measure selected SWIFT tree")?;
                drop((measurement, parsed, storage));
                Ok(())
            },
        );
        let CodecError::ResourceLimit(limit) = error else {
            panic!("measurement refusal");
        };
        limit.used
    };
    assert_eq!(live_storage(true), live_storage(false));
}

#[test]
fn swift_suppressed_roster_skips_geometry_indexes_and_provenance() {
    let mut root = Entity {
        class: crate::swift::ROOT_CLASS.into(),
        ..Entity::default()
    };
    let mut annotation = super::entity("GdtFlatness");
    annotation.integers.insert("IsSuppressed".into(), 1);
    root.annotations
        .references
        .push(super::reference("A10", "GdtFlatness"));
    root.annotations.entities.push(annotation);
    assert_geometry_indexes_skipped(&root, None);
    let source = source_with_swift_root(&root);
    let scan = crate::test_support::container::scan(&source);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&source, &arena, &policy).unwrap();
    let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        ResourceDimension::WorkUnits,
        "format SWIFT PMI identity",
        None,
    );
    let mut provenance = cadmpeg_ir::annotations::Annotations::default();
    let projection = annotations(&ctx, &scan, &mut provenance, None).unwrap();
    assert!(projection.annotations.is_empty());
    assert!(provenance.provenance.is_empty());
    ctx.finish_session().unwrap();
}

#[test]
fn swift_explicit_nominal_skips_native_pattern_index() {
    use super::identity::pattern_hole_features;
    let mut root = Entity {
        class: crate::swift::ROOT_CLASS.into(),
        ..Entity::default()
    };
    let mut annotation = super::entity("GdtDiameter");
    annotation.doubles.insert("Nominal".into(), 4.0);
    annotation
        .features
        .references
        .push(super::reference("F10", "GdtCylinder"));
    root.annotations
        .references
        .push(super::reference("A10", "GdtDiameter"));
    root.annotations.entities.push(annotation);
    root.features
        .references
        .push(super::reference("F10", "GdtCylinder"));
    root.features.entities.push(super::entity("GdtCylinder"));
    let source = source_with_swift_root(&root);
    let scan = crate::test_support::container::scan(&source);
    let features = pattern_hole_features();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&source, &arena, &policy).unwrap();
    let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        ResourceDimension::WorkUnits,
        "swift pattern hole candidates",
        None,
    );
    let projection = annotations(
        &ctx,
        &scan,
        &mut cadmpeg_ir::annotations::Annotations::default(),
        Some(crate::swift::GeometryProjectionInputs {
            topology: crate::swift::PrimaryTopology {
                bodies: &[],
                faces: &[],
                edges: &[],
                vertices: &[],
            },
            face_bridge_sequences: &[],
            edge_use_sequences: &[],
            vertex_use_sequences: &[],
            features: &features,
        }),
    )
    .unwrap();
    assert_eq!(projection.annotations.len(), 1);
    assert_eq!(
        dimension_nominal(&projection.annotations, "A10"),
        length(4.0)
    );
    ctx.finish_session().unwrap();
}

#[test]
fn swift_suppressed_roster_skips_populated_feature_index() {
    let mut root = Entity {
        class: crate::swift::ROOT_CLASS.into(),
        ..Entity::default()
    };
    for index in 0..64 {
        root.features
            .references
            .push(super::reference(&format!("F{index}"), "GdtPlane"));
        root.features.entities.push(super::entity("GdtPlane"));
    }
    let mut annotation = super::entity("GdtFlatness");
    annotation.integers.insert("IsSuppressed".into(), 1);
    annotation
        .features
        .references
        .push(super::reference("F0", "GdtPlane"));
    root.annotations
        .references
        .push(super::reference("A10", "GdtFlatness"));
    root.annotations.entities.push(annotation);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        ResourceDimension::WorkUnits,
        "index SWIFT features",
        None,
    );
    let projection =
        crate::swift::project_with_topology(&ctx, &root, None, &crate::swift::RenderedDimensions::Borrowed(&[]), None, None).unwrap();
    assert!(projection.annotations.is_empty());
    assert!(projection.unsupported.is_empty());
    drop(projection);
    ctx.finish_session().unwrap();
}

#[test]
fn swift_discarded_stream_roots_release_their_storage() {
    let root = Entity {
        class: crate::swift::ROOT_CLASS.into(),
        ..Entity::default()
    };
    let mut payload = Vec::new();
    encode_entity(&root, &mut payload);
    let mut source = source_with_swift_root(&root);
    source.extend(crate::test_support::container::make_block(
        0x41,
        "SWIFT/OtherSchema",
        &payload,
    ));
    let scan = crate::test_support::container::scan(&source);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 1024 * 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(crate::swift::scan_root(&ctx, &scan).unwrap().is_none());
    let all_scratch = ctx
        .reserve_scoped(
            policy.limits.max_materialized_bytes,
            "reuse SWIFT stream discovery scratch",
        )
        .unwrap();
    drop(all_scratch);
    ctx.finish_session().unwrap();
}

#[test]
fn swift_second_root_skips_unused_descendant_index() {
    let mut root = Entity {
        class: crate::swift::ROOT_CLASS.into(),
        ..Entity::default()
    };
    let mut payload = Vec::new();
    encode_entity(&root, &mut payload);
    for index in 0..64 {
        root.features
            .references
            .push(super::reference(&format!("F{index}"), "GdtPlane"));
        root.features.entities.push(super::entity("GdtPlane"));
    }
    encode_entity(&root, &mut payload);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        ResourceDimension::WorkUnits,
        "index validated SWIFT features",
        Some(64),
    );
    assert!(parse_unique_root(&ctx, &payload).unwrap().is_none());
    ctx.finish_session().unwrap();
}

#[test]
fn swift_rejected_annotation_gates_skip_populated_feature_index() {
    for class in ["GdtDatum", "GdtAngleBetween", "GdtUnrecognized"] {
        let mut root = Entity {
            class: crate::swift::ROOT_CLASS.into(),
            ..Entity::default()
        };
        for index in 0..64 {
            root.features
                .references
                .push(super::reference(&format!("F{index}"), "GdtPlane"));
            root.features.entities.push(super::entity("GdtPlane"));
        }
        let mut annotation = super::entity(class);
        annotation
            .features
            .references
            .push(super::reference("F0", "GdtPlane"));
        root.annotations
            .references
            .push(super::reference("A10", class));
        root.annotations.entities.push(annotation);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            ResourceDimension::WorkUnits,
            "index SWIFT features",
            None,
        );
        {
            let result = crate::swift::project_with_topology(&ctx, &root, None, &crate::swift::RenderedDimensions::Borrowed(&[]), None, None);
            if class == "GdtDatum" {
                let Err(CodecError::Malformed(message)) = result else {
                    panic!("missing datum identifier is malformed");
                };
                assert!(message.contains("missing or empty DatumIdentifier"));
            } else {
                let projection = result.unwrap();
                assert!(projection.annotations.is_empty());
                assert_eq!(projection.unsupported.get(class), Some(&1));
                drop(projection);
            }
        }
        ctx.finish_session().unwrap();
    }
}

#[test]
fn swift_datum_reuses_its_admitted_pmi_identity() {
    let mut datum = super::entity("GdtDatum");
    datum.strings.insert("DatumIdentifier".into(), "A".into());
    let features = BTreeMap::<&str, &Entity>::new();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let id = crate::swift::pmi_id_charged(&ctx, "A10").unwrap().unwrap();
    let geometry = crate::swift::ProjectionGeometry::new(&ctx, None, None, None);
    let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        ResourceDimension::WorkUnits,
        "format SWIFT PMI identity",
        None,
    );
    let annotation = crate::swift::project_datum(&ctx, id, &datum, || Ok(&features), &geometry)
        .unwrap()
        .unwrap();
    assert_eq!(annotation.id, crate::swift::pmi_id("A10").unwrap());
    assert!(
        matches!(annotation.definition, PmiDefinition::Datum { identification } if identification == "A")
    );
    drop(geometry);
    ctx.finish_session().unwrap();
}

#[test]
fn swift_unbindable_cad_identifiers_skip_topology_indexes() {
    use cadmpeg_ir::ids::BodyId;
    use cadmpeg_ir::topology::Body;
    let bodies = vec![
        Body {
            id: BodyId::mint("sldprt:brep:body#11").unwrap(),
            kind: cadmpeg_ir::topology::BodyKind::default(),
            regions: Vec::new(),
            transform: None,
            name: None,
            color: None,
            visible: None,
        };
        64
    ];
    let sequences = vec![(11, 1); 64];
    for (identifier, empty) in [
        ("invalid", false),
        (":11", false),
        ("schema:", false),
        ("schema:not-a-number", false),
        ("schema:11", true),
    ] {
        let mut root = Entity {
            class: crate::swift::ROOT_CLASS.into(),
            ..Entity::default()
        };
        let mut feature = super::entity("GdtCylinder");
        let mut cad = super::entity("CadRef");
        cad.strings
            .insert("CadIdentifier".into(), identifier.into());
        feature.related.push(RelatedObject {
            name: "Cad".into(),
            class: cad.class.clone(),
            entity: cad,
        });
        root.features
            .references
            .push(super::reference("F10", "GdtCylinder"));
        root.features.entities.push(feature);
        let mut annotation = super::entity("GdtDiameter");
        annotation.doubles.insert("Nominal".into(), 4.0);
        annotation
            .features
            .references
            .push(super::reference("F10", "GdtCylinder"));
        root.annotations
            .references
            .push(super::reference("A10", "GdtDiameter"));
        root.annotations.entities.push(annotation);
        let source = source_with_swift_root(&root);
        let scan = crate::test_support::container::scan(&source);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        let (ctx, _) = DecodeContext::from_root_bytes(&source, &arena, &policy).unwrap();
        let operation = if empty {
            "index SWIFT face sequence"
        } else {
            "index SWIFT body identities"
        };
        let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            ResourceDimension::WorkUnits,
            operation,
            Some(64),
        );
        let projection = annotations(
            &ctx,
            &scan,
            &mut cadmpeg_ir::annotations::Annotations::default(),
            Some(crate::swift::GeometryProjectionInputs {
                topology: crate::swift::PrimaryTopology {
                    bodies: if empty { &[] } else { &bodies },
                    faces: &[],
                    edges: &[],
                    vertices: &[],
                },
                face_bridge_sequences: if empty { &sequences } else { &[] },
                edge_use_sequences: &[],
                vertex_use_sequences: &[],
                features: &[],
            }),
        )
        .unwrap();
        assert_eq!(projection.annotations.len(), 1);
        assert_eq!(
            dimension_nominal(&projection.annotations, "A10"),
            length(4.0)
        );
        assert!(
            matches!(projection.annotations.first().expect("one annotation").targets.as_slice(), [PmiTarget::ShapeAspect { source_id }] if source_id.as_str() == "F10")
        );
        ctx.finish_session().unwrap();
    }
}

#[test]
fn swift_dimension_reuses_its_admitted_pmi_identity() {
    use cadmpeg_core::decode::refusal_probe::RefusalProbe;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let root = super::entity("GdtAnalysis");
    let counterbores = crate::swift::Counterbores::new(&ctx, &root).unwrap();
    let mut entity = super::entity("GdtDiameter");
    entity.doubles.insert("Nominal".into(), 4.0);
    let id = crate::swift::pmi_id_charged(&ctx, "A10").unwrap().unwrap();
    let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, "format SWIFT PMI identity", None);
    let geometry = crate::swift::ProjectionGeometry::new(&ctx, None, None, None);
    let features = BTreeMap::new();
    let annotation = crate::swift::project_dimension(
        &ctx,
        id,
        &counterbores,
        &entity,
        || Ok(&features),
        &geometry,
        &crate::swift::RenderedDimensions::Borrowed(&[]),
    ).unwrap().unwrap();
    assert_eq!(annotation.id, crate::swift::pmi_id("A10").unwrap());
    assert_eq!(dimension_nominal(&[annotation], "A10"), length(4.0));
    drop((geometry, counterbores));
    ctx.finish_session().unwrap();
}

fn append_rendered_literal(payload: &mut Vec<u8>, text: &str) {
    payload.extend_from_slice(&[0xff, 0xfe, 0xff]);
    payload.push(u8::try_from(text.encode_utf16().count()).unwrap());
    payload.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
}

#[test]
fn swift_root_ambiguity_does_not_prepay_marker_free_tails() {
    let root = Entity { class: crate::swift::ROOT_CLASS.into(), ..Entity::default() };
    let mut prefix = Vec::new();
    encode_entity(&root, &mut prefix);
    encode_entity(&root, &mut prefix);
    let mut prefix_work = None;
    for tail in [32, 16_384] {
        let mut payload = prefix.clone();
        payload.extend(vec![0; tail]);
        crate::test_support::work_refusal_at("scan SWIFT entity roots", |ctx| {
            parse_unique_root(ctx, &payload).map(|_| ())
        });
        let error = crate::test_support::work_refusal_at("measure SWIFT root scan work", |ctx| {
            assert!(parse_unique_root(ctx, &payload)?.is_none());
            ctx.copy_retained_text("x", "measure SWIFT root scan work")
        });
        let CodecError::ResourceLimit(limit) = error else { panic!("root scan measurement"); };
        if let Some(expected) = prefix_work {
            assert_eq!(limit.used, expected, "marker-free bytes after ambiguity are unvisited");
        } else {
            prefix_work = Some(limit.used);
        }
    }
}

#[test]
fn swift_unqueried_rendered_literals_are_not_decoded() {
    use cadmpeg_core::decode::refusal_probe::RefusalProbe;
    for roster in ["empty", "suppressed", "explicit"] {
        let mut root = Entity { class: crate::swift::ROOT_CLASS.into(), ..Entity::default() };
        if roster != "empty" {
            let mut annotation = super::entity("GdtDiameter");
            annotation.doubles.insert("Nominal".into(), 4.0);
            if roster == "suppressed" {
                annotation.integers.insert("IsSuppressed".into(), 1);
            }
            root.annotations.references.push(super::reference("A10", "GdtDiameter"));
            root.annotations.entities.push(annotation);
        }
        let mut payload = Vec::new();
        encode_entity(&root, &mut payload);
        for _ in 0..64 { append_rendered_literal(&mut payload, "<MOD-DIAM> 4.000"); }
        let mut source = crate::test_support::container::synthetic_sldprt();
        source.extend(crate::test_support::container::make_block(0x40, "SWIFT/Schema", &payload));
        let scan = crate::test_support::container::scan(&source);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, "scan SWIFT rendered literals", None);
        let result = annotations(&ctx, &scan, &mut cadmpeg_ir::annotations::Annotations::default(), None).unwrap();
        if roster == "explicit" {
            assert_eq!(result.annotations.len(), 1);
            assert_eq!(dimension_nominal(&result.annotations, "A10"), length(4.0));
        } else {
            assert!(result.annotations.is_empty());
        }
        assert!(result.unsupported_loss.is_none());
        ctx.finish_session().unwrap();
    }
}

#[test]
fn swift_ambiguous_stream_roots_skip_rendered_literals_and_name_copies() {
    use cadmpeg_core::decode::refusal_probe::RefusalProbe;
    let root = Entity { class: crate::swift::ROOT_CLASS.into(), ..Entity::default() };
    let mut payload = Vec::new();
    encode_entity(&root, &mut payload);
    for _ in 0..64 { append_rendered_literal(&mut payload, "<MOD-DIAM> 4.000"); }
    let mut source = crate::test_support::container::synthetic_sldprt();
    for (tag, name) in [(0x40, "SWIFT/Schema"), (0x41, "SWIFT/OtherSchema")] {
        source.extend(crate::test_support::container::make_block(tag, name, &payload));
    }
    let scan = crate::test_support::container::scan(&source);
    for operation in ["scan SWIFT rendered literals", "copy SWIFT source stream name"] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
        assert!(crate::swift::scan_root(&ctx, &scan).unwrap().is_none());
        ctx.finish_session().unwrap();
    }
}

#[test]
fn swift_rendered_dimension_cache_is_built_at_first_query_and_reused() {
    use cadmpeg_core::decode::refusal_probe::RefusalProbe;
    let mut payload = Vec::new();
    append_rendered_literal(&mut payload, "<MOD-DIAM> 4.000");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 1024 * 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let cache = crate::swift::RenderedDimensions::Stream { payload: &payload, values: std::cell::OnceCell::new() };
    assert_eq!(cache.get(&ctx).unwrap().len(), 1);
    let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, "scan SWIFT rendered literals", None);
    assert_eq!(cache.get(&ctx).unwrap().first().unwrap().value.get(), 4.0);
    drop(cache);
    let reuse = ctx.reserve_scoped(policy.limits.max_materialized_bytes, "reuse rendered cache storage").unwrap();
    drop(reuse);
    ctx.finish_session().unwrap();
}

#[test]
fn swift_rendered_literals_hold_only_surviving_vector_storage() {
    let mut payload = Vec::new();
    for _ in 0..64 { append_rendered_literal(&mut payload, "<MOD-DIAM> 4.000"); }
    let live_storage = |decode| {
        let arena = DecodeArena::new();
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::MaterializedBytes, "measure rendered dimension storage", |cap| {
                let mut policy = DecodePolicy::service();
                policy.limits.max_materialized_bytes = cap;
                policy.limits.max_retained_bytes = 0;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
                let (values, storage) = ctx.with_scoped_storage("rendered dimension test output", || {
                    if decode {
                        crate::swift::rendered_dimensions(&ctx, &payload)
                    } else {
                        let mut values = Vec::new();
                        for _ in 0..64 {
                            ctx.push_vec(&mut values, crate::swift::RenderedDimension {
                                kind: crate::swift::RenderedDimensionKind::Diameter,
                                value: crate::swift::PositiveReal::new(4.0).unwrap(), decimal_places: 3,
                            }, "expected rendered dimensions")?;
                        }
                        Ok(values)
                    }
                })?;
                assert_eq!(values.len(), 64);
                assert!(values.iter().all(|value| value.value.get() == 4.0 && value.decimal_places == 3));
                let measurement = ctx.reserve_scoped(u64::MAX / 2, "measure rendered dimension storage")?;
                drop((measurement, values, storage));
                Ok(())
            });
        let CodecError::ResourceLimit(limit) = error else { panic!("rendered storage measurement"); };
        limit.used
    };
    assert_eq!(live_storage(true), live_storage(false));
}

#[test]
fn swift_unsupported_annotations_release_their_admitted_identity() {
    let mut root = Entity { class: crate::swift::ROOT_CLASS.into(), ..Entity::default() };
    root.annotations.references.push(super::reference("A10", "GdtUnrecognized"));
    root.annotations.entities.push(super::entity("GdtUnrecognized"));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 1024 * 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result = crate::swift::project_with_topology(&ctx, &root, None,
        &crate::swift::RenderedDimensions::Borrowed(&[]), None, None).unwrap();
    assert!(result.annotations.is_empty());
    assert_eq!(result.unsupported.get("GdtUnrecognized"), Some(&1));
    drop(result);
    let reuse = ctx.reserve_scoped(policy.limits.max_materialized_bytes, "reuse unsupported identity storage").unwrap();
    drop(reuse);
    ctx.finish_session().unwrap();
}

#[test]
fn swift_lower_tier_identity_retains_only_its_final_text() {
    let reference = super::reference("A10", "GdtSurfaceProfile");
    let mut entity = super::entity("GdtSurfaceProfile");
    entity.doubles.insert("ToleranceLowerTier".into(), 0.25);
    let arena = DecodeArena::new();
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RetainedBytes, "format SWIFT lower-tier ID", |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
            let geometry = crate::swift::ProjectionGeometry::new(&ctx, None, None, None);
            project_lower_profile_tier(&ctx, &reference, &entity, &BTreeMap::new(), &geometry)
        });
    let CodecError::ResourceLimit(limit) = error else { panic!("lower-tier retained boundary"); };
    assert_eq!(limit.used, 0, "the validation identity is temporary");
}

#[test]
fn swift_root_marker_search_skips_unvisited_tails() {
    let mut prefix_work = None;
    for tail in [0, 96] {
        let name = format!("SWIFT/Schema{}", "x".repeat(tail));
        let mut payload = crate::swift::ROOT_CLASS.as_bytes().to_vec();
        payload.extend(vec![0; tail.checked_mul(128).expect("test tail fits usize")]);
        let mut source = crate::test_support::container::synthetic_sldprt();
        source.extend(crate::test_support::container::make_block(0x40, &name, &payload));
        let scan = crate::test_support::container::scan(&source);
        for operation in ["find SWIFT schema section name", "scan SWIFT root class marker"] {
            let error = crate::test_support::work_refusal_at(operation, |ctx| {
                crate::swift::has_root_marker(ctx, &scan)
            });
            let CodecError::ResourceLimit(limit) = error else { panic!("marker search boundary"); };
            assert_eq!(limit.additional, 1);
        }
        let error = crate::test_support::work_refusal_at("measure SWIFT marker work", |ctx| {
            assert!(crate::swift::has_root_marker(ctx, &scan)?);
            ctx.copy_retained_text("x", "measure SWIFT marker work")
        });
        let CodecError::ResourceLimit(limit) = error else { panic!("marker search measurement"); };
        if let Some(expected) = prefix_work { assert_eq!(limit.used, expected); }
        else { prefix_work = Some(limit.used); }
    }
}

#[test]
fn swift_ambiguous_annotation_fallback_skips_unvisited_tails() {
    let root = Entity { class: crate::swift::ROOT_CLASS.into(), ..Entity::default() };
    let mut prefix = Vec::new();
    encode_entity(&root, &mut prefix);
    encode_entity(&root, &mut prefix);
    let mut prefix_work = None;
    for tail in [32, 16_384] {
        let mut payload = prefix.clone();
        payload.extend(vec![0; tail]);
        let mut source = crate::test_support::container::synthetic_sldprt();
        source.extend(crate::test_support::container::make_block(0x40, "SWIFT/Schema", &payload));
        let scan = crate::test_support::container::scan(&source);
        crate::test_support::work_refusal_at("scan SWIFT root class marker", |ctx| {
            annotations(ctx, &scan, &mut cadmpeg_ir::annotations::Annotations::default(), None)
        });
        let error = crate::test_support::work_refusal_at("measure ambiguous SWIFT annotation work", |ctx| {
            let result = annotations(ctx, &scan, &mut cadmpeg_ir::annotations::Annotations::default(), None)?;
            assert!(result.annotations.is_empty());
            assert!(result.unsupported_loss.as_deref().unwrap().contains("GdtAnalysisGraphUnresolved"));
            ctx.copy_retained_text("x", "measure ambiguous SWIFT annotation work")
        });
        let CodecError::ResourceLimit(limit) = error else { panic!("annotation work measurement"); };
        if let Some(expected) = prefix_work { assert_eq!(limit.used, expected); }
        else { prefix_work = Some(limit.used); }
    }
}

#[test]
fn swift_rejected_targets_release_partial_output_and_skip_datum_copy() {
    let mut datum = super::entity("GdtDatum");
    datum.strings.insert("DatumIdentifier".into(), "A".repeat(128));
    datum.features.references = vec![super::reference("F10", "GdtPlane"), super::reference(" ", "GdtPlane")];
    let features = BTreeMap::new();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 1024 * 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let geometry = crate::swift::ProjectionGeometry::new(&ctx, None, None, None);
    let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        ResourceDimension::WorkUnits, "copy SWIFT datum identifier", None,
    );
    assert!(crate::swift::project_datum(&ctx, pmi_id("A10").unwrap(), &datum, || Ok(&features), &geometry).unwrap().is_none());
    let reuse = ctx.reserve_scoped(policy.limits.max_materialized_bytes, "reuse rejected target storage").unwrap();
    drop(reuse);
    drop(geometry);
    ctx.finish_session().unwrap();
}

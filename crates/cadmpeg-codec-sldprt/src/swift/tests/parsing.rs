use super::*;

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
        bytes.extend_from_slice(&(entity.strings.len() as u32).to_le_bytes());
        for (key, value) in &entity.strings {
            put_pstr(bytes, key);
            put_pstr(bytes, value);
        }
        put_pstr(bytes, "EndStrings");
    }
    if !entity.integers.is_empty() {
        put_pstr(bytes, "Integers");
        bytes.extend_from_slice(&(entity.integers.len() as u32).to_le_bytes());
        for (key, value) in &entity.integers {
            put_pstr(bytes, key);
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        put_pstr(bytes, "EndIntegers");
    }
    if !entity.doubles.is_empty() {
        put_pstr(bytes, "Doubles");
        bytes.extend_from_slice(&(entity.doubles.len() as u32).to_le_bytes());
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
        bytes.extend_from_slice(&(entity.related.len() as u32).to_le_bytes());
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
    bytes.extend_from_slice(&(section.references.as_slice().len() as u32).to_le_bytes());
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

#[test]
fn parses_and_projects_semantic_graph() {
    let parsed = parse_unique_root(&encoded_root()).expect("synthetic SWIFT root");
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
                source_id: cadmpeg_ir::products::NonEmptyString::new("F20")
                    .expect("nonempty source identity")
            },
            PmiTarget::ShapeAspect {
                source_id: cadmpeg_ir::products::NonEmptyString::new("F21")
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
    let PmiDefinition::Dimension {
        nominal: Some(nominal),
        ..
    } = &angle.definition
    else {
        panic!("angular definition");
    };
    assert_eq!(
        *nominal,
        PmiValue::new(0.0, PmiQuantity::Angle).expect("finite angle")
    );
}

#[test]
fn rejects_ambiguous_root_and_impossible_count() {
    let encoded = encoded_root();
    let mut duplicate = encoded.clone();
    duplicate.extend_from_slice(&encoded);
    assert_eq!(parse_unique_root(&duplicate), None);

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
    assert_eq!(parse_unique_root(&malformed), None);
}

#[test]
fn malformed_reference_identity_returns_decode_loss() {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    for id in ["", "A 10"] {
        let mut root = semantic_root();
        root.annotations.references.first_mut().unwrap().id = id.into();
        let mut payload = Vec::new();
        encode_entity(&root, &mut payload);
        let parsed = parse_unique_root(&payload).unwrap();
        assert_eq!(parsed.annotations.references.first().unwrap().id, id);
        let mut bytes = crate::test_support::synthetic_sldprt();
        bytes.extend(crate::test_support::make_block(
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

// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;

use super::{generation, read, ChildOccurrence, DefinitionRole, Generation, Placement, Structure};
use crate::loss::PvzLossCode;
use crate::test_support::pvs::{
    definition, geometry, inline_atom, member, occurrence, property_set, uleb, Node, Place,
};
use crate::test_support::{assembly_structure, SCALED_TURN};

fn decode(bytes: &[u8]) -> Result<Structure, CodecError> {
    let ctx = cadmpeg_test_support::service_decode_context();
    read(&ctx, View::over_retained(bytes))
}

fn loss_codes(structure: &Structure) -> Vec<cadmpeg_ir::report::loss::LossKind> {
    structure
        .residue
        .losses()
        .into_iter()
        .map(|loss| loss.code)
        .collect()
}

/// A structure stream root over `definitions`.
fn structure_stream(definitions: Vec<Node>) -> Vec<u8> {
    Node::list(0x02, definitions).encode()
}

#[test]
fn each_signature_selects_its_generation() {
    for expected in Generation::ALL {
        let mut member = expected.signature().as_bytes().to_vec();
        member.extend_from_slice(b"\0\x01 body");
        assert_eq!(
            generation(View::over_retained(&member)),
            Some(expected),
            "{}",
            expected.signature()
        );
    }
}

#[test]
fn other_or_short_leading_bytes_select_no_generation() {
    for member in [
        b"PV-PVS0300 body".as_slice(),
        b"PV-PVS0306 body",
        b"PV-PVS030",
        b"DIV-OL09BL body",
        b"",
    ] {
        assert_eq!(generation(View::over_retained(member)), None, "{member:?}");
    }
}

#[test]
fn the_assembly_decodes_to_its_definitions_occurrences_and_properties() {
    let structure = decode(&assembly_structure("PV-PVS0303")).unwrap();
    let names: Vec<_> = structure
        .definitions
        .iter()
        .map(|definition| (definition.name.as_str(), definition.geometry.as_deref()))
        .collect();
    assert_eq!(
        names,
        [
            ("TOP.ASM", None),
            ("BRACKET.PRT", Some("bracket.ol")),
            ("PLATE.PRT", Some("plate.ol")),
        ]
    );
    assert_eq!(
        structure.definitions[0].occurrences,
        [
            ChildOccurrence {
                definition: 1,
                placement: Placement {
                    translation: Some([0.1, 0.0, 0.0]),
                    basis: None,
                },
                feature_id: "41".to_owned(),
            },
            ChildOccurrence {
                definition: 1,
                placement: Placement {
                    translation: Some([0.0, 0.2, 0.0]),
                    basis: Some(SCALED_TURN),
                },
                feature_id: "42".to_owned(),
            },
            ChildOccurrence {
                definition: 2,
                placement: Placement::default(),
                feature_id: "43".to_owned(),
            },
        ]
    );
    let properties: Vec<_> = structure
        .definitions
        .iter()
        .map(|definition| {
            definition
                .properties
                .iter()
                .map(|property| (property.name.as_str(), property.value.as_str()))
                .collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(
        properties,
        [
            vec![("PART_NUMBER", "TOP-001")],
            vec![],
            vec![("MATERIAL", "STEEL")],
        ]
    );
    assert_eq!(
        loss_codes(&structure),
        [PvzLossCode::StructureBoundsNotTransferred.kind()]
    );
}

#[test]
fn every_placement_encoding_reads_its_values() {
    let basis = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
    let stream = structure_stream(vec![
        definition(
            "ROOT",
            vec![
                occurrence(1, Place::Identity, "a"),
                occurrence(1, Place::Translation([1.0, 2.0, 3.0]), "b"),
                occurrence(1, Place::Basis(basis), "c"),
                occurrence(1, Place::Full([4.0, 5.0, 6.0], basis), ""),
            ],
        ),
        definition("LEAF", vec![]),
    ]);
    let structure = decode(&member("PV-PVS0301", &[(2, "", stream)])).unwrap();
    let placements: Vec<_> = structure.definitions[0]
        .occurrences
        .iter()
        .map(|occurrence| occurrence.placement)
        .collect();
    assert_eq!(
        placements,
        [
            Placement::default(),
            Placement {
                translation: Some([1.0, 2.0, 3.0]),
                basis: None
            },
            Placement {
                translation: None,
                basis: Some(basis)
            },
            Placement {
                translation: Some([4.0, 5.0, 6.0]),
                basis: Some(basis)
            },
        ]
    );
    assert!(structure.definitions[1].occurrences.is_empty());
    assert!(loss_codes(&structure).is_empty());
}

#[test]
fn unsettled_occurrence_and_definition_bytes_are_counted() {
    let mut flagged = uleb(1);
    flagged.extend(inline_atom("f"));
    let mut extended = uleb(1);
    extended.extend(inline_atom("g"));
    extended.extend([0x01, 0x08, b'A', 0x00]);
    let mut undefined = uleb(1);
    undefined.extend(inline_atom("h"));
    let mut typed_name = inline_atom("TYPED");
    typed_name.extend([7, 7, 3, 3]);
    let stream = structure_stream(vec![
        definition(
            "ROOT",
            vec![
                Node::typed(0x21, 0x68, flagged),
                Node::wide(0x21, 0x0208, extended),
                Node::typed(0x21, 0x09, undefined),
                Node::list(0x2e, vec![Node::typed(0x2f, 0x05, vec![0; 24])]),
            ],
        ),
        Node::typed_parent(0x20, 0x1e, typed_name, vec![]),
    ]);
    let structure = decode(&member("PV-PVS0302", &[(2, "", stream)])).unwrap();
    let root = &structure.definitions[0];
    assert_eq!(root.occurrences.len(), 2);
    assert_eq!(root.occurrences[0].placement, Placement::default());
    assert_eq!(root.occurrences[1].feature_id, "g");
    assert_eq!(structure.definitions[1].name, "TYPED");
    assert_eq!(structure.definitions[1].role, DefinitionRole::Component);
    assert_eq!(
        loss_codes(&structure),
        [
            PvzLossCode::StructureElementUndecoded.kind(),
            PvzLossCode::StructureDefinitionAttributes.kind(),
            PvzLossCode::StructureOccurrenceAttributes.kind(),
            PvzLossCode::StructureOccurrenceUndecoded.kind(),
        ]
    );
}

#[test]
fn an_illustration_root_has_its_role() {
    let mut payload = inline_atom("Illustrations");
    payload.push(0);
    let stream = structure_stream(vec![
        Node::typed_parent(
            0x20,
            0x01,
            payload,
            vec![occurrence(1, Place::Identity, "")],
        ),
        definition("TOP.ASM", vec![]),
    ]);
    let structure = decode(&member("PV-PVS0304", &[(2, "", stream)])).unwrap();
    assert_eq!(
        structure.definitions[0].role,
        DefinitionRole::IllustrationRoot
    );
}

#[test]
fn the_property_cursor_steps_over_definitions() {
    let stream = structure_stream(vec![
        definition("A", vec![]),
        definition("B", vec![]),
        definition("C", vec![]),
    ]);
    let general = Node::list(
        0x03,
        vec![
            property_set(2, &[("k", "b")]),
            property_set(1, &[("k", "c"), ("m", "n")]),
        ],
    )
    .encode();
    let named = Node::list(0x03, vec![property_set(1, &[("k", "a")])]).encode();
    let structure = decode(&member(
        "PV-PVS0305",
        &[(2, "", stream), (3, "", general), (3, "sbom", named)],
    ))
    .unwrap();
    let found: Vec<Vec<(String, String, String)>> = structure
        .definitions
        .iter()
        .map(|definition| {
            definition
                .properties
                .iter()
                .map(|p| (p.stream.clone(), p.name.clone(), p.value.clone()))
                .collect()
        })
        .collect();
    let row = |stream: &str, name: &str, value: &str| {
        (stream.to_owned(), name.to_owned(), value.to_owned())
    };
    assert_eq!(
        found,
        [
            vec![row("sbom", "k", "a")],
            vec![row("", "k", "b")],
            vec![row("", "k", "c"), row("", "m", "n")],
        ]
    );
}

#[test]
fn a_failed_property_stream_leaves_the_structure_and_is_reported() {
    let stream = structure_stream(vec![definition("A", vec![])]);
    let past_end = Node::list(0x03, vec![property_set(2, &[("k", "v")])]).encode();
    let structure = decode(&member(
        "PV-PVS0301",
        &[(2, "", stream), (3, "", past_end), (9, "extra", vec![1, 2])],
    ))
    .unwrap();
    assert!(structure.definitions[0].properties.is_empty());
    assert_eq!(
        loss_codes(&structure),
        [
            PvzLossCode::StructurePropertyStreamMalformed.kind(),
            PvzLossCode::StructureStreamUndecoded.kind(),
        ]
    );
}

#[test]
fn a_member_whose_directory_does_not_tile_the_member_is_refused() {
    let stream = structure_stream(vec![definition("A", vec![])]);
    let good = member("PV-PVS0301", &[(2, "", stream)]);

    let mut trailing = good.clone();
    trailing.push(0);
    let mut early = good.clone();
    early[10] += 1;
    let truncated = good[..good.len() - 1].to_vec();
    let no_structure = member("PV-PVS0301", &[]);
    let mut other_codec = good.clone();
    other_codec[19] = 2;

    for bad in [trailing, early, truncated, no_structure, other_codec] {
        assert!(decode(&bad).is_err());
    }
    assert!(decode(b"not a structure member").is_err());
}

#[test]
fn an_occurrence_of_a_missing_definition_is_refused() {
    let stream = structure_stream(vec![definition(
        "A",
        vec![occurrence(5, Place::Identity, "")],
    )]);
    assert!(decode(&member("PV-PVS0301", &[(2, "", stream)])).is_err());
}

#[test]
fn a_second_geometry_element_is_refused() {
    let stream = structure_stream(vec![definition(
        "A",
        vec![
            geometry("a.ol", [0.0; 6]),
            Node::leaf(0x23, inline_atom("b.ol")),
        ],
    )]);
    assert!(decode(&member("PV-PVS0301", &[(2, "", stream)])).is_err());
}

#[test]
fn the_xml_form_maps_to_the_same_model() {
    let xml = br#"<PV_FILE type="PVS" version="0302">
<section_index><internal_section type="2"/><internal_section type="3"/></section_index>
<section_structure>
  <component name="TOP.ASM">
    <component_instance index="1" translation="0.1,0,0" id="41"/>
    <component_instance index="1" translation="0,0.2,0" orientation="0,2,0,-2,0,0,0,0,2" id="42"/>
    <illustration3d_source file_name="a.pvi" name="Figure 1"/>
  </component>
  <component name="BRACKET.PRT">
    <shape_source file_name="bracket.ol" bbox="0,0,0,0.01,0.01,0.01"/>
  </component>
</section_structure>
<section_properties>
  <property_component_ref index_offset="2"><property name="MATERIAL" value="STEEL"/></property_component_ref>
</section_properties>
</PV_FILE>"#;
    let structure = decode(xml).unwrap();
    assert_eq!(structure.definitions.len(), 2);
    assert_eq!(
        structure.definitions[1].geometry.as_deref(),
        Some("bracket.ol")
    );
    assert_eq!(
        structure.definitions[0].occurrences[1],
        ChildOccurrence {
            definition: 1,
            placement: Placement {
                translation: Some([0.0, 0.2, 0.0]),
                basis: Some(SCALED_TURN),
            },
            feature_id: "42".to_owned(),
        }
    );
    assert_eq!(structure.definitions[1].properties[0].value, "STEEL");
    assert_eq!(
        loss_codes(&structure),
        [
            PvzLossCode::StructureElementUndecoded.kind(),
            PvzLossCode::StructureBoundsNotTransferred.kind(),
        ]
    );
    assert!(decode(br#"<PV_FILE type="PVS"><section_structure><component name="A"><component_instance index="4"/></component></section_structure></PV_FILE>"#).is_err());
    assert!(decode(br#"<PV_FILE type="PVS"><section_structure><component name="A"><component_instance index="0" translation="1,2"/></component></section_structure></PV_FILE>"#).is_err());
    assert!(decode(b"<PV_FILE type=\"PVS\">").is_err());
}

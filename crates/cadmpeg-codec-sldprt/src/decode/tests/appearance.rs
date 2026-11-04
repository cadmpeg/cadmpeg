// SPDX-License-Identifier: Apache-2.0
//! Scoped appearance projection tests.

#[test]
fn scoped_face_binding_does_not_expose_source_offset() {
    let definition = crate::appearance::AppearanceDefinition {
        name: "Paint".into(),
        color: cadmpeg_ir::topology::Color::from_rgba8(1, 2, 3, 255),
        packed_hex: "01020300".into(),
        source_name: cadmpeg_ir::StreamName::try_from("VisualStates".to_owned()).unwrap(),
        record_offset: 42,
    };
    let face = cadmpeg_ir::ids::FaceId::mint("sldprt:brep:face#1").unwrap();
    let appearance = cadmpeg_ir::ids::AppearanceId::mint("sldprt:appearance:visual#1").unwrap();
    let binding = crate::decode::scoped_face_binding(&face, appearance, &definition, 1, 0);
    assert_eq!(binding.source_entity_id, None);
}

// SPDX-License-Identifier: Apache-2.0
//! Nested reusable source definitions keep their neutral placement tree.

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::products::OccurrenceParent;
use std::io::Cursor;

#[test]
fn nested_subfigures_project_names_and_local_placements() {
    let decoded = crate::IgesCodec
        .decode(
            &mut Cursor::new(
                crate::test_support::test_drawing_and_trimming::nested_subfigure_file(),
            ),
            &DecodeOptions::default(),
        )
        .expect("nested subfigure fixture decodes");
    let ir = decoded.ir();
    assert_eq!(ir.model.product_definitions.len(), 2);
    let labels = ir
        .model
        .product_definitions
        .iter()
        .map(|definition| {
            definition
                .label
                .as_deref()
                .expect("subfigure definition has an authored label")
        })
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        labels,
        std::collections::BTreeSet::from(["CHILD", "PARENT"])
    );
    assert_eq!(ir.model.occurrences.len(), 2);
    let root = ir
        .model
        .occurrences
        .iter()
        .find(|occurrence| matches!(occurrence.parent, OccurrenceParent::Root {}))
        .expect("root product occurrence exists");
    let child = ir.model.occurrences.iter().find(|occurrence| matches!(&occurrence.parent, OccurrenceParent::Occurrence { occurrence: parent } if *parent == root.id)).expect("nested child occurrence refers to the root");
    assert_eq!(
        root.transform
            .expect("authored root placement")
            .affine_rows(),
        [
            [2.0, 0.0, 0.0, 10.0],
            [0.0, 2.0, 0.0, 20.0],
            [0.0, 0.0, 2.0, 30.0]
        ]
    );
    assert_eq!(
        child
            .transform
            .expect("source occurrence placement")
            .affine_rows(),
        [
            [0.5, 0.0, 0.0, 1.0],
            [0.0, 0.5, 0.0, 2.0],
            [0.0, 0.0, 0.5, 3.0]
        ]
    );
    for record in &ir
        .native
        .namespace("iges")
        .expect("decoded IGES native namespace")
        .arenas()["product_occurrences"]
    {
        let world = record
            .field("world_transform")
            .expect("retained native world placement");
        assert_eq!(
            world
                .as_array()
                .expect("native table value is an array")
                .len(),
            3
        );
    }
}

#[test]
fn solid_instances_project_local_occurrences_of_their_solid_definition() {
    let decoded = crate::IgesCodec
        .decode(
            &mut Cursor::new(crate::test_support::test_solids_and_structure::solid_instance_file()),
            &DecodeOptions::default(),
        )
        .expect("solid instance fixture decodes");
    let ir = decoded.ir();
    assert_eq!(ir.model.product_definitions.len(), 1);
    let definition = &ir.model.product_definitions[0];
    assert_eq!(definition.id.as_str(), "iges:model:product#D1");
    assert!(matches!(
        definition.kind,
        cadmpeg_ir::products::ProductDefinitionKind::Part
    ));
    assert!(definition.bodies.is_empty());
    assert_eq!(
        definition.native_ref.as_deref(),
        Some("iges:solid:primitive#D1")
    );
    assert_eq!(ir.model.occurrences.len(), 1);
    let occurrence = &ir.model.occurrences[0];
    assert_eq!(occurrence.id.as_str(), "iges:model:occurrence#D1:D3");
    let cadmpeg_ir::products::PrototypeReference::Local {
        definition: prototype,
    } = &occurrence.prototype
    else {
        panic!("solid instance takes the Local prototype");
    };
    assert_eq!(prototype.as_str(), "iges:model:product#D1");
    assert!(matches!(occurrence.parent, OccurrenceParent::Root {}));
    assert_eq!(occurrence.ordinal, 0);
    assert!(occurrence.transform.is_none());
    assert_eq!(
        occurrence.native_ref.as_deref(),
        Some("iges:product:solid-instance#D3")
    );
    assert!(decoded.report().losses.is_empty());
    let validation =
        cadmpeg_ir::validate_neutral(ir, Vec::new()).expect("validate the neutral model");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn solid_assemblies_project_reusable_part_definitions() {
    let decoded = crate::IgesCodec
        .decode(
            &mut Cursor::new(crate::test_support::test_solids_and_structure::solid_assembly_file()),
            &DecodeOptions::default(),
        )
        .expect("solid assembly fixture decodes");
    let ir = decoded.ir();
    assert_eq!(ir.model.product_definitions.len(), 1);
    let definition = &ir.model.product_definitions[0];
    assert_eq!(definition.id.as_str(), "iges:model:product#D7");
    assert_eq!(definition.label.as_deref(), Some("ASSEMBLY"));
    assert_eq!(
        definition.native_ref.as_deref(),
        Some("iges:product:solid-assembly#D7")
    );
    assert!(ir.model.occurrences.is_empty());
    assert!(decoded.report().losses.is_empty());
    let validation =
        cadmpeg_ir::validate_neutral(ir, Vec::new()).expect("validate the neutral model");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn external_references_project_external_occurrences_without_resolution() {
    use cadmpeg_ir::products::{ExternalDocument, PrototypeReference};

    let decoded = crate::IgesCodec
        .decode(
            &mut Cursor::new(
                crate::test_support::test_solids_and_structure::external_reference_forms_file(),
            ),
            &DecodeOptions::default(),
        )
        .expect("external reference fixture decodes");
    let ir = decoded.ir();
    assert!(ir.model.product_definitions.is_empty());
    assert_eq!(ir.model.occurrences.len(), 5);
    let expected = [
        ("iges:model:occurrence#D1", "part.igs", Some("BRACKET")),
        ("iges:model:occurrence#D3", "assembly.igs", None),
        ("iges:model:occurrence#D5", "sheet.igs", Some("FLANGE1")),
    ];
    for (index, (id, path, object)) in expected.into_iter().enumerate() {
        let occurrence = &ir.model.occurrences[index];
        assert_eq!(occurrence.id.as_str(), id);
        assert_eq!(occurrence.ordinal, index as u32);
        assert!(matches!(occurrence.parent, OccurrenceParent::Root {}));
        let PrototypeReference::External {
            document,
            object: actual,
        } = &occurrence.prototype
        else {
            panic!("external reference takes the External prototype");
        };
        let ExternalDocument::Path { path: actual_path } = document else {
            panic!("file-backed external reference names its path");
        };
        assert_eq!(actual_path.as_str(), path);
        assert_eq!(actual.as_deref(), object);
    }
    let native_definition = &ir.model.occurrences[3];
    let PrototypeReference::External { document, object } = &native_definition.prototype else {
        panic!("native definition takes the External prototype");
    };
    assert!(matches!(document, ExternalDocument::Missing {}));
    assert_eq!(object.as_deref(), Some("MOTOR"));
    let library = &ir.model.occurrences[4];
    let PrototypeReference::External { document, object } = &library.prototype else {
        panic!("library definition takes the External prototype");
    };
    let ExternalDocument::DocumentId { document_id } = document else {
        panic!("library definition names its library");
    };
    assert_eq!(document_id.as_str(), "DEVICES");
    assert_eq!(object.as_deref(), Some("RELAY"));
    assert!(decoded.report().losses.is_empty());
    let validation =
        cadmpeg_ir::validate_neutral(ir, Vec::new()).expect("validate the neutral model");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn assembly_members_place_their_instances_with_the_item_placement() {
    use crate::test_support::test_owned::{owned_test_file, OwnedTestEntity};

    let decoded = crate::IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                OwnedTestEntity {
                    entity_type: 158,
                    form: 0,
                    label: "SPHERE".into(),
                    status: "00000000",
                    parameters: "158,1,0,0,0;".into(),
                },
                OwnedTestEntity {
                    entity_type: 430,
                    form: 0,
                    label: "INSTANCE".into(),
                    status: "00000000",
                    parameters: "430,1;".into(),
                },
                OwnedTestEntity {
                    entity_type: 124,
                    form: 0,
                    label: "MOVE".into(),
                    status: "00010000",
                    parameters: "124,1,0,0,10,0,1,0,0,0,0,1,0;".into(),
                },
                OwnedTestEntity {
                    entity_type: 184,
                    form: 0,
                    label: "ASSEMBLY".into(),
                    status: "00000200",
                    parameters: "184,1,3,5;".into(),
                },
            ])),
            &DecodeOptions::default(),
        )
        .expect("nested solid instance decodes");
    let ir = decoded.ir();
    assert_eq!(ir.model.product_definitions.len(), 2);
    assert_eq!(ir.model.occurrences.len(), 2);
    let member = ir
        .model
        .occurrences
        .iter()
        .find(|occurrence| occurrence.id.as_str() == "iges:model:occurrence#D7:D3")
        .expect("assembly member occurrence exists");
    let cadmpeg_ir::products::PrototypeReference::Local { definition } = &member.prototype else {
        panic!("assembly member takes the Local prototype");
    };
    assert_eq!(definition.as_str(), "iges:model:product#D1");
    assert_eq!(
        member.transform.expect("item placement").affine_rows(),
        [
            [1.0, 0.0, 0.0, 10.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0]
        ]
    );
    assert!(decoded.report().losses.is_empty());
    let validation =
        cadmpeg_ir::validate_neutral(ir, Vec::new()).expect("validate the neutral model");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

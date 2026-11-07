// SPDX-License-Identifier: Apache-2.0
//! Product and occurrence names and root placements in STEP export.

use std::io::Cursor;

use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::examples::unit_cube;
use cadmpeg_ir::CadIr;

use crate::export::write_step;
use crate::{StepCodec, StepSchema, StepWriteOptions};

fn unnamed_definition(
    id: &str,
    bodies: Vec<cadmpeg_ir::ids::BodyId>,
) -> cadmpeg_ir::products::ProductDefinition {
    cadmpeg_ir::products::ProductDefinition {
        sketches: Vec::default(),
        id: cadmpeg_ir::ids::ProductDefinitionId::mint(id).expect("identity grammar"),
        kind: cadmpeg_ir::products::ProductDefinitionKind::Part,
        source_name: None,
        label: None,
        description: None,
        part_number: None,
        bom_properties: std::collections::BTreeMap::default(),
        bodies,
        native_ref: None,
    }
}

fn unnamed_occurrence(
    id: &str,
    definition: &str,
    parent: Option<&str>,
) -> cadmpeg_ir::products::Occurrence {
    cadmpeg_ir::products::Occurrence {
        id: cadmpeg_ir::ids::OccurrenceId::mint(id).expect("identity grammar"),
        prototype: cadmpeg_ir::products::PrototypeReference::Local {
            definition: cadmpeg_ir::ids::ProductDefinitionId::mint(definition)
                .expect("identity grammar"),
        },
        parent: parent.map_or(cadmpeg_ir::products::OccurrenceParent::Root {}, |parent| {
            cadmpeg_ir::products::OccurrenceParent::Occurrence {
                occurrence: cadmpeg_ir::ids::OccurrenceId::mint(parent).expect("identity grammar"),
            }
        }),
        ordinal: 0,
        transform: Some(cadmpeg_ir::transform::Transform::identity()),
        linked_prototype: None,
        name: None,
        visible: None,
        link: None,
        native_ref: None,
    }
}

/// The name parameter of every `keyword` record, in emission order.
fn record_names(step: &str, keyword: &str) -> Vec<String> {
    let opening = format!(" = {keyword}('");
    step.lines()
        .filter_map(|line| line.split_once(&opening))
        .filter_map(|(_, rest)| rest.split('\'').nth(2).map(str::to_owned))
        .collect()
}

fn write_named(ir: &CadIr, product_name: &str) -> String {
    let options = StepWriteOptions {
        product_name: product_name.into(),
        ..StepWriteOptions::default()
    };
    let mut output = Vec::new();
    write_step(ir, &mut output, StepSchema::Ap242Edition3, &options).expect("write products");
    String::from_utf8(output).expect("STEP text")
}

#[test]
fn writer_names_unnamed_products_and_occurrences_distinctly() {
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    ir.model.bodies[0].name = Some("Body1".into());
    let body = ir.model.bodies[0].id.clone();
    ir.model.product_definitions = vec![
        unnamed_definition("test:model:product-definition#assembly", Vec::new()),
        unnamed_definition("test:model:product-definition#part-0", vec![body]),
        unnamed_definition("test:model:product-definition#part-1", Vec::new()),
    ];
    ir.model.occurrences = vec![
        unnamed_occurrence(
            "test:model:occurrence#root",
            "test:model:product-definition#assembly",
            None,
        ),
        unnamed_occurrence(
            "test:model:occurrence#occ-0",
            "test:model:product-definition#part-0",
            Some("test:model:occurrence#root"),
        ),
        unnamed_occurrence(
            "test:model:occurrence#occ-1",
            "test:model:product-definition#part-1",
            Some("test:model:occurrence#root"),
        ),
        unnamed_occurrence(
            "test:model:occurrence#occ-2",
            "test:model:product-definition#part-0",
            Some("test:model:occurrence#root"),
        ),
    ];
    for model in ["Bracket model", "Other model"] {
        let step = write_named(&ir, model);
        assert!(step.contains(&format!("FILE_NAME('{model}',")), "{step}");
        assert_eq!(record_names(&step, "PRODUCT"), [model, "part-0", "part-1"]);
        assert_eq!(
            record_names(&step, "NEXT_ASSEMBLY_USAGE_OCCURRENCE"),
            ["occ-0", "occ-1", "occ-2"]
        );
    }

    let decoded = StepCodec::default()
        .decode(
            &mut Cursor::new(write_named(&ir, "Bracket model")),
            &DecodeOptions::default(),
        )
        .expect("decode unnamed assembly");
    let mut labels = decoded
        .ir()
        .model
        .product_definitions
        .iter()
        .map(|product| product.label.as_deref())
        .collect::<Vec<_>>();
    labels.sort_unstable();
    assert_eq!(
        labels,
        [Some("Bracket model"), Some("part-0"), Some("part-1")]
    );
    let mut names = decoded
        .ir()
        .model
        .occurrences
        .iter()
        .filter_map(|occurrence| occurrence.name.as_deref())
        .collect::<Vec<_>>();
    names.sort_unstable();
    assert_eq!(names, ["occ-0", "occ-1", "occ-2"]);
}

#[test]
fn unplaced_occurrences_round_trip_as_product_usages_without_placements() {
    use cadmpeg_ir::products::OccurrenceParent;

    let mut ir = unit_cube().expect("cube");
    ir.model.product_definitions = vec![
        unnamed_definition("test:model:product#assembly", Vec::new()),
        unnamed_definition(
            "test:model:product#part",
            vec![ir.model.bodies[0].id.clone()],
        ),
        unnamed_definition("test:model:product#child", Vec::new()),
    ];
    let mut root = unnamed_occurrence(
        "test:model:occurrence#root",
        "test:model:product#assembly",
        None,
    );
    root.name = Some("Unplaced root".into());
    root.transform = None;
    let mut child = unnamed_occurrence(
        "test:model:occurrence#child",
        "test:model:product#part",
        Some("test:model:occurrence#root"),
    );
    child.name = Some("Unplaced child".into());
    child.transform = None;
    let mut grandchild = unnamed_occurrence(
        "test:model:occurrence#grandchild",
        "test:model:product#child",
        Some("test:model:occurrence#child"),
    );
    grandchild.name = Some("Placed grandchild".into());
    ir.model.occurrences = vec![root, child, grandchild];
    let mut bytes = Vec::new();
    let report = write_step(
        &ir,
        &mut bytes,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .expect("write assembly");
    assert!(!report
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::StepLossCode::OccurrencePlacementUnknown.kind()));
    let text = std::str::from_utf8(&bytes).expect("STEP text");
    assert_eq!(
        text.matches(" = NEXT_ASSEMBLY_USAGE_OCCURRENCE(").count(),
        3
    );
    assert_eq!(
        text.matches(" = CONTEXT_DEPENDENT_SHAPE_REPRESENTATION(")
            .count(),
        1
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode assembly");
    let find = |name| {
        decoded
            .ir()
            .model
            .occurrences
            .iter()
            .find(|occurrence| occurrence.name.as_deref() == Some(name))
            .expect("named occurrence")
    };
    let root = find("Unplaced root");
    let child = find("Unplaced child");
    let grandchild = find("Placed grandchild");
    assert!(root.transform.is_none());
    assert!(child.transform.is_none());
    assert!(grandchild.transform.is_some());
    assert!(
        matches!(&child.parent, OccurrenceParent::Occurrence { occurrence } if occurrence == &root.id)
    );
    assert!(
        matches!(&grandchild.parent, OccurrenceParent::Occurrence { occurrence } if occurrence == &child.id)
    );
}

#[test]
fn unplaced_external_paths_round_trip_as_product_definition_references() {
    use cadmpeg_ir::products::{ExternalDocument, PrototypeReference};

    let mut ir = CadIr::empty();
    for (index, object) in [Some("part"), None].into_iter().enumerate() {
        let mut occurrence = unnamed_occurrence(
            &format!("test:model:occurrence#{index}"),
            "test:model:product#unused",
            None,
        );
        occurrence.prototype = PrototypeReference::External {
            document: ExternalDocument::path("assembly/part.step"),
            object: object.map(str::to_owned),
        };
        occurrence.name = Some(format!("External {index}"));
        occurrence.ordinal = u32::try_from(index).unwrap();
        occurrence.transform = None;
        ir.model.occurrences.push(occurrence);
    }
    let mut bytes = Vec::new();
    let report = write_step(
        &ir,
        &mut bytes,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .expect("write external occurrences");
    assert!(!report
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::StepLossCode::OccurrenceExternalProduct.kind()));
    assert!(ir
        .model
        .occurrences
        .iter()
        .all(
            |occurrence| report.losses.iter().any(|loss| loss.code.local_code()
                == "record.residual-unavailable"
                && loss.message.contains(occurrence.id.as_str()))
        ));
    let text = std::str::from_utf8(&bytes).unwrap();
    assert_eq!(text.matches(" = PRODUCT_DEFINITION_REFERENCE(").count(), 2);
    assert_eq!(
        text.matches(" = NEXT_ASSEMBLY_USAGE_OCCURRENCE(").count(),
        2
    );
    assert!(!text.contains("CONTEXT_DEPENDENT_SHAPE_REPRESENTATION("));
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode external occurrences");
    for original in &ir.model.occurrences {
        let written = decoded
            .ir()
            .model
            .occurrences
            .iter()
            .find(|row| row.name == original.name)
            .expect("occurrence");
        assert_eq!(written.prototype, original.prototype);
        assert!(written.transform.is_none());
    }
}

#[test]
fn writer_leaves_ambiguous_fallback_names_empty() {
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    let body = ir.model.bodies[0].id.clone();
    ir.model.product_definitions = vec![
        unnamed_definition("test:first:product-definition#part", vec![body]),
        unnamed_definition("test:second:product-definition#part", Vec::new()),
    ];
    let step = write_named(&ir, "Bracket model");
    assert_eq!(record_names(&step, "PRODUCT"), ["", ""]);

    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(step), &DecodeOptions::default())
        .expect("decode unnamed products");
    assert_eq!(decoded.ir().model.product_definitions.len(), 2);
    for product in &decoded.ir().model.product_definitions {
        assert_eq!(product.label, None);
        assert_eq!(product.source_name, None);
    }
}

#[test]
fn writer_names_the_model_product_from_options_on_both_paths() {
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    ir.model.bodies[0].name = Some("Body1".into());
    let step = write_named(&ir, "Bracket model");
    assert_eq!(record_names(&step, "PRODUCT"), ["Bracket model"]);

    ir.model.product_definitions = vec![unnamed_definition(
        "test:model:product-definition#part-0",
        vec![ir.model.bodies[0].id.clone()],
    )];
    ir.model.occurrences = vec![unnamed_occurrence(
        "test:model:occurrence#root",
        "test:model:product-definition#part-0",
        None,
    )];
    let step = write_named(&ir, "Bracket model");
    assert_eq!(record_names(&step, "PRODUCT"), ["part-0", "Bracket model"]);
}

#[test]
fn writer_names_a_product_by_its_part_number() {
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    let mut definition = unnamed_definition(
        "test:model:product-definition#part-0",
        vec![ir.model.bodies[0].id.clone()],
    );
    definition.part_number = Some("P-1001".into());
    ir.model.product_definitions = vec![definition];
    ir.model.occurrences = vec![unnamed_occurrence(
        "test:model:occurrence#root",
        "test:model:product-definition#part-0",
        None,
    )];
    let step = write_named(&ir, "Bracket model")
        .replace("PRODUCT('P-1001','P-1001',", "PRODUCT('P-1001','',");
    assert_eq!(record_names(&step, "PRODUCT"), ["", "Bracket model"]);

    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(step), &DecodeOptions::default())
        .expect("decode product named by its part number");
    let product = decoded
        .ir()
        .model
        .product_definitions
        .first()
        .expect("decoded product");
    assert_eq!(product.label, None);
    assert_eq!(product.part_number.as_deref(), Some("P-1001"));
    let step = write_named(decoded.ir(), "Bracket model");
    assert_eq!(record_names(&step, "PRODUCT"), ["P-1001", "Bracket model"]);
}

/// World translation of every decoded use of a product labeled `label`.
fn world_translations(ir: &CadIr, label: &str) -> Vec<[f64; 3]> {
    let definition = ir
        .model
        .product_definitions
        .iter()
        .find(|product| product.label.as_deref() == Some(label))
        .map(|product| product.id.clone())
        .expect("labeled product");
    let mut translations = ir
        .model
        .occurrences
        .iter()
        .filter(|occurrence| {
            matches!(&occurrence.prototype, cadmpeg_ir::products::PrototypeReference::Local { definition: used } if *used == definition)
        })
        .map(|occurrence| {
            let mut world = occurrence
                .effective_transform()
                .expect("rigid occurrence")
                .expect("placed occurrence");
            let mut parent = occurrence.parent.clone();
            while let cadmpeg_ir::products::OccurrenceParent::Occurrence { occurrence } = parent {
                let owner = ir
                    .model
                    .occurrences
                    .iter()
                    .find(|candidate| candidate.id == occurrence)
                    .expect("parent occurrence");
                world = owner
                    .effective_transform()
                    .expect("rigid parent")
                    .expect("placed parent")
                    .compose(world)
                    .expect("composed placement");
                parent = owner.parent.clone();
            }
            let rows = world.rows();
            [rows[0][3], rows[1][3], rows[2][3]]
        })
        .collect::<Vec<_>>();
    translations.sort_by(|left, right| left.partial_cmp(right).expect("finite"));
    translations
}

#[test]
fn writer_places_rigid_root_occurrences_under_the_model_product() {
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    let body = ir.model.bodies[0].id.clone();
    let mut part = unnamed_definition("test:model:product-definition#part-0", vec![body]);
    part.label = Some("Cube".into());
    ir.model.product_definitions = vec![part];
    let mut moved = unnamed_occurrence(
        "test:model:occurrence#moved",
        "test:model:product-definition#part-0",
        None,
    );
    moved.transform = Some(
        cadmpeg_ir::transform::Transform::affine([
            [1.0, 0.0, 0.0, 10.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ])
        .expect("finite translation"),
    );
    let mut scaled = unnamed_occurrence(
        "test:model:occurrence#scaled",
        "test:model:product-definition#part-0",
        None,
    );
    scaled.transform = Some(
        cadmpeg_ir::transform::Transform::affine([
            [2.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ])
        .expect("finite scale"),
    );
    ir.model.occurrences = vec![
        unnamed_occurrence(
            "test:model:occurrence#origin",
            "test:model:product-definition#part-0",
            None,
        ),
        moved,
        scaled,
    ];
    let options = StepWriteOptions {
        product_name: "Bracket model".into(),
        ..StepWriteOptions::default()
    };
    let mut output = Vec::new();
    let report = write_step(&ir, &mut output, StepSchema::Ap242Edition3, &options)
        .expect("write placed roots");
    let step = String::from_utf8(output).expect("STEP text");
    assert_eq!(record_names(&step, "PRODUCT"), ["Cube", "Bracket model"]);
    assert_eq!(
        report
            .losses
            .iter()
            .filter(|loss| {
                loss.code
                    == crate::loss::StepLossCode::RootOccurrencePlacementNotRepresentable.kind()
            })
            .count(),
        1
    );

    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(step), &DecodeOptions::default())
        .expect("decode placed roots");
    assert_eq!(
        world_translations(decoded.ir(), "Cube"),
        [[0.0, 0.0, 0.0], [10.0, 0.0, 0.0]]
    );
}

#[test]
fn writer_reports_a_placed_external_root_occurrence_only_as_external() {
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    let body = ir.model.bodies[0].id.clone();
    ir.model.product_definitions = vec![unnamed_definition(
        "test:model:product-definition#part-0",
        vec![body],
    )];
    let translation = cadmpeg_ir::transform::Transform::affine([
        [1.0, 0.0, 0.0, 10.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ])
    .expect("finite translation");
    let mut local = unnamed_occurrence(
        "test:model:occurrence#local",
        "test:model:product-definition#part-0",
        None,
    );
    local.transform = Some(translation);
    let mut external = unnamed_occurrence(
        "test:model:occurrence#external",
        "test:model:product-definition#part-0",
        None,
    );
    external.prototype = cadmpeg_ir::products::PrototypeReference::External {
        document: cadmpeg_ir::products::ExternalDocument::path("other.FCStd"),
        object: Some("Box".into()),
    };
    external.transform = Some(translation);
    ir.model.occurrences = vec![local, external];

    let report = write_step(
        &ir,
        &mut Vec::new(),
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .expect("write external root");
    let reported =
        |code: crate::loss::StepLossCode| report.losses.iter().any(|loss| loss.code == code.kind());
    assert!(reported(
        crate::loss::StepLossCode::OccurrenceExternalProduct
    ));
    assert!(!reported(
        crate::loss::StepLossCode::RootOccurrencePlacementNotRepresentable
    ));
}

fn document_attribute(id: &str, name: &str, value: &str) -> SourceAttribute {
    SourceAttribute {
        id: id.try_into().expect("identity grammar"),
        native_ref: None,
        target: AttributeTarget::Document,
        name: name.into(),
        values: vec![Some(AttributeValue::String(value.into()))],
    }
}

#[test]
fn writer_folds_document_attributes_into_file_name() {
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    ir.model.attributes.extend([
        document_attribute("test:model:attribute#label", "Label", "Bracket"),
        document_attribute("test:model:attribute#author", "author", "anne"),
        document_attribute("test:model:attribute#created", "CreatedBy", "anne"),
        document_attribute("test:model:attribute#modified", "LastModifiedBy", "beth"),
        document_attribute("test:model:attribute#company", "Company", "acme"),
        document_attribute(
            "test:model:attribute#created-on",
            "CreationDate",
            "2026-07-14T00:00:00",
        ),
        document_attribute("test:model:attribute#system", "system_id", "origin"),
        document_attribute("test:model:attribute#comment", "Comment", "kept as a loss"),
    ]);
    let mut output = Vec::new();
    let report = write_step(
        &ir,
        &mut output,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .expect("write file name fields");
    let step = String::from_utf8(output).expect("STEP text");
    assert!(
        step.contains("FILE_NAME('Bracket','2026-07-14T00:00:00',('anne','beth'),('acme'),'cadmpeg-step','origin','');"),
        "{step}"
    );
    assert!(
        report
            .losses
            .iter()
            .filter(|loss| loss.code == crate::loss::StepLossCode::SourceAttributeNotWritten.kind())
            .any(|loss| loss.message.contains("test:model:attribute#comment")),
        "unmapped document attribute keeps its loss: {report:?}"
    );
    assert!(
        report
            .losses
            .iter()
            .all(|loss| !loss.message.contains("test:model:attribute#label")),
        "folded attributes keep no loss: {report:?}"
    );

    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(step), &DecodeOptions::default())
        .expect("decode folded header");
    let held = |name: &str| {
        decoded
            .ir()
            .model
            .attributes
            .iter()
            .filter(|attribute| {
                attribute.target == AttributeTarget::Document && attribute.name == name
            })
            .map(|attribute| {
                let [Some(AttributeValue::String(value))] = attribute.values.as_slice() else {
                    panic!("document attribute {name} holds one string");
                };
                value.clone()
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(held("Label"), ["Bracket"]);
    assert_eq!(held("author"), ["anne"]);
    assert_eq!(held("CreatedBy"), ["anne"]);
    assert_eq!(held("LastModifiedBy"), ["beth"]);
    assert_eq!(held("Company"), ["acme"]);
}

#[test]
fn document_author_losses_name_values_not_projected_back() {
    let mut ir = unit_cube().expect("unit cube");
    ir.model.attributes.extend([
        document_attribute("test:model:attribute#first", "author", "anne"),
        document_attribute("test:model:attribute#second", "author", "beth"),
        document_attribute("test:model:attribute#third", "LastModifiedBy", "chris"),
    ]);
    let mut output = Vec::new();
    let report = write_step(
        &ir,
        &mut output,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .expect("write header");
    let losses: Vec<_> = report
        .losses
        .iter()
        .filter(|loss| loss.code == crate::loss::StepLossCode::SourceAttributeNotWritten.kind())
        .collect();
    assert!(losses
        .iter()
        .any(|loss| loss.message.contains("test:model:attribute#second")));
    assert!(losses
        .iter()
        .any(|loss| loss.message.contains("test:model:attribute#third")));
    assert!(losses
        .iter()
        .all(|loss| !loss.message.contains("test:model:attribute#first")));
}

#[test]
fn iges_document_timestamp_uses_step_syntax_and_names_the_normalization() {
    let mut ir = unit_cube().expect("unit cube");
    let id = "test:model:attribute#date";
    ir.model
        .attributes
        .push(document_attribute(id, "generation_date", "20260714.000000"));
    let mut bytes = Vec::new();
    let report = write_step(
        &ir,
        &mut bytes,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .expect("write date");
    assert!(report.losses.iter().any(|loss| {
        loss.code == crate::loss::StepLossCode::TimestampNormalized.kind()
            && loss.message.contains(id)
    }));
    assert!(!report.losses.iter().any(|loss| {
        loss.code == crate::loss::StepLossCode::SourceAttributeNotWritten.kind()
            && loss.message.contains(id)
    }));
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode date");
    let date = decoded
        .ir()
        .model
        .attributes
        .iter()
        .find(|attribute| attribute.name == "generation_date")
        .expect("document date");
    assert_eq!(
        date.values.as_slice(),
        [Some(AttributeValue::String("2026-07-14T00:00:00".into()))]
    );
}
#[test]
fn omitted_occurrence_link_metadata_names_each_occurrence() {
    let mut ir = unit_cube().expect("cube");
    ir.model.product_definitions = vec![
        unnamed_definition("test:model:product-definition#assembly", Vec::new()),
        unnamed_definition(
            "test:model:product-definition#part",
            vec![ir.model.bodies[0].id.clone()],
        ),
    ];
    let mut root = unnamed_occurrence(
        "test:model:occurrence#root",
        "test:model:product-definition#assembly",
        None,
    );
    let mut child = unnamed_occurrence(
        "test:model:occurrence#child",
        "test:model:product-definition#part",
        Some("test:model:occurrence#root"),
    );
    root.link = cadmpeg_ir::products::LinkState::new(Vec::new(), None, Some(false), None);
    child.link = cadmpeg_ir::products::LinkState::new(Vec::new(), None, Some(true), None);
    ir.model.occurrences = vec![root, child];
    let mut bytes = Vec::new();
    let report = write_step(
        &ir,
        &mut bytes,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .expect("write assembly");
    for id in ["test:model:occurrence#root", "test:model:occurrence#child"] {
        assert!(report.losses.iter().any(|loss| loss.code
            == crate::loss::StepLossCode::OccurrenceMetadataOmitted.kind()
            && loss.message.contains(id)));
    }
}
#[test]
fn writer_preserves_an_identity_placed_root_occurrence() {
    let mut ir = unit_cube().expect("cube");
    ir.model.product_definitions = vec![unnamed_definition(
        "test:model:product-definition#part",
        vec![ir.model.bodies[0].id.clone()],
    )];
    let mut root = unnamed_occurrence(
        "test:model:occurrence#root",
        "test:model:product-definition#part",
        None,
    );
    root.name = Some("Root part".into());
    ir.model.occurrences = vec![root];
    let text = write_named(&ir, "Assembly");
    assert_eq!(
        record_names(&text, "NEXT_ASSEMBLY_USAGE_OCCURRENCE"),
        ["Root part"]
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(text), &DecodeOptions::default())
        .expect("decode assembly");
    assert!(decoded.ir().model.occurrences.iter().any(|occurrence| {
        occurrence.name.as_deref() == Some("Root part")
            && occurrence.effective_transform().ok().flatten()
                == Some(cadmpeg_ir::transform::Transform::identity())
    }));
}

#[test]
fn product_scalar_attributes_are_step_properties_of_their_owner() {
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    let body = ir.model.bodies[0].id.clone();
    let first = "test:model:product-definition#first";
    let second = "test:model:product-definition#second";
    ir.model.product_definitions = vec![
        unnamed_definition(first, vec![body]),
        unnamed_definition(second, vec![]),
    ];
    let attribute = |id: &str, target, name: &str, value: AttributeValue| SourceAttribute {
        id: id.try_into().expect("identity grammar"),
        native_ref: None,
        target,
        name: name.into(),
        values: vec![Some(value)],
    };
    ir.model.attributes.push(attribute(
        "test:model:attribute#first",
        AttributeTarget::ProductDefinition(first.try_into().unwrap()),
        "Finish",
        AttributeValue::String("brushed".into()),
    ));
    ir.model.attributes.push(attribute(
        "test:model:attribute#second",
        AttributeTarget::Record {
            arena: cadmpeg_ir::document::ArenaName::parse("product_definitions").unwrap(),
            id: second.into(),
        },
        "Material",
        AttributeValue::String("steel".into()),
    ));
    let owner = AttributeTarget::ProductDefinition(first.try_into().unwrap());
    ir.model.attributes.push(attribute(
        "test:model:attribute#count",
        owner.clone(),
        "Count",
        AttributeValue::Integer(7),
    ));
    ir.model.attributes.push(attribute(
        "test:model:attribute#ratio",
        owner.clone(),
        "Ratio",
        AttributeValue::float(1.25).unwrap(),
    ));
    ir.model.attributes.push(attribute(
        "test:model:attribute#enabled",
        owner,
        "Enabled",
        AttributeValue::Boolean(true),
    ));
    let mut output = Vec::new();
    let report = write_step(
        &ir,
        &mut output,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .unwrap();
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(&output), &DecodeOptions::default())
        .expect("decode product properties");
    for (name, expected) in [
        ("Finish", AttributeValue::String("brushed".into())),
        ("Material", AttributeValue::String("steel".into())),
        ("Count", AttributeValue::Integer(7)),
        ("Enabled", AttributeValue::Boolean(true)),
    ] {
        assert!(
            decoded.ir().model.attributes.iter().any(|attribute| {
                attribute.name == name
                    && matches!(attribute.target, AttributeTarget::ProductDefinition(_))
                    && attribute.values == [Some(expected.clone())]
            }),
            "missing product property {name}"
        );
    }
    assert!(decoded.ir().model.attributes.iter().any(|attribute| {
        attribute.name == "Ratio"
            && matches!(attribute.target, AttributeTarget::ProductDefinition(_))
            && matches!(attribute.values.as_slice(), [Some(AttributeValue::Float(value))] if (value.get() - 1.25).abs() < 1.0e-12)
    }));
    let text = String::from_utf8(output).unwrap();
    for (name, item_text, product) in [
        (
            "Finish",
            "DESCRIPTIVE_REPRESENTATION_ITEM('','brushed')",
            first,
        ),
        (
            "Material",
            "DESCRIPTIVE_REPRESENTATION_ITEM('','steel')",
            second,
        ),
        ("Count", "INTEGER_REPRESENTATION_ITEM('',7)", first),
        ("Ratio", "REAL_REPRESENTATION_ITEM('',1.25)", first),
        ("Enabled", "BOOLEAN_REPRESENTATION_ITEM('',.T.)", first),
    ] {
        let prefix = format!("PROPERTY_DEFINITION('{name}','',#");
        let property = text
            .lines()
            .find(|line| line.contains(&prefix))
            .expect("property definition");
        let owner = property
            .split_once(&prefix)
            .unwrap()
            .1
            .split(')')
            .next()
            .unwrap();
        assert!(text.lines().any(|line| line
            .starts_with(&format!("#{owner} = PRODUCT_DEFINITION("))
            && line.contains(product)));
        let property_id = property.split_once(" = ").unwrap().0;
        let relation_prefix = format!("PROPERTY_DEFINITION_REPRESENTATION({property_id},#");
        let relation = text
            .lines()
            .find(|line| line.contains(&relation_prefix))
            .expect("property representation link");
        let representation = relation
            .split_once(&relation_prefix)
            .unwrap()
            .1
            .split(')')
            .next()
            .unwrap();
        let rep_prefix = format!("#{representation} = REPRESENTATION('',(#");
        let rep = text
            .lines()
            .find(|line| line.starts_with(&rep_prefix))
            .expect("property representation");
        let item = rep
            .strip_prefix(&rep_prefix)
            .unwrap()
            .split(')')
            .next()
            .unwrap();
        assert!(text.contains(&format!("#{item} = {item_text}")));
    }
    assert!(!report
        .losses
        .iter()
        .any(|loss| loss.message.contains("test:model:attribute#first")));
    assert!(report.losses.iter().any(|loss| {
        loss.code == crate::loss::StepLossCode::SourceAttributeTargetNormalized.kind()
            && loss.message.contains("test:model:attribute#second")
    }));
}

#[test]
fn older_step_schema_reports_unwritten_product_boolean() {
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    let product = "test:model:product-definition#part";
    ir.model.product_definitions = vec![unnamed_definition(
        product,
        vec![ir.model.bodies[0].id.clone()],
    )];
    ir.model.attributes.push(SourceAttribute {
        id: "test:model:attribute#flag".try_into().unwrap(),
        native_ref: None,
        target: AttributeTarget::ProductDefinition(product.try_into().unwrap()),
        name: "Enabled".into(),
        values: vec![Some(AttributeValue::Boolean(true))],
    });
    let mut output = Vec::new();
    let report = write_step(
        &ir,
        &mut output,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .unwrap();
    let text = String::from_utf8(output).unwrap();
    assert!(!text.contains("BOOLEAN_REPRESENTATION_ITEM"));
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.message.contains("test:model:attribute#flag")));
}

#[test]
fn distinct_product_source_name_is_reported_with_its_occurrence() {
    let mut ir = unit_cube().expect("cube");
    let product_id = "test:model:product-definition#part";
    let mut product = unnamed_definition(product_id, vec![ir.model.bodies[0].id.clone()]);
    product.label = Some("Displayed label".into());
    product.source_name = Some("Source name".into());
    ir.model.product_definitions = vec![product];
    let occurrence = unnamed_occurrence("test:model:occurrence#part", product_id, None);
    let occurrence_id = occurrence.id.clone();
    ir.model.occurrences = vec![occurrence];
    let mut output = Vec::new();
    let report = write_step(
        &ir,
        &mut output,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .expect("write assembly");
    assert!(report.losses.iter().any(|loss| {
        loss.code == crate::loss::StepLossCode::ProductSourceNameReduced.kind()
            && loss.message.contains(product_id)
            && loss.message.contains(occurrence_id.as_str())
    }));
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode product");
    assert!(decoded
        .ir()
        .model
        .product_definitions
        .iter()
        .any(|product| product.source_name.as_deref() == Some("Displayed label")));
}

#[test]
fn assembly_occurrences_keep_sibling_order() {
    let mut ir = unit_cube().expect("cube");
    ir.model.product_definitions = vec![
        unnamed_definition("test:model:product-definition#assembly", Vec::new()),
        unnamed_definition(
            "test:model:product-definition#part",
            vec![ir.model.bodies[0].id.clone()],
        ),
    ];
    let root = unnamed_occurrence(
        "test:model:occurrence#root",
        "test:model:product-definition#assembly",
        None,
    );
    let mut later = unnamed_occurrence(
        "test:model:occurrence#later",
        "test:model:product-definition#part",
        Some("test:model:occurrence#root"),
    );
    later.name = Some("Later".into());
    later.ordinal = 1;
    let mut earlier = unnamed_occurrence(
        "test:model:occurrence#earlier",
        "test:model:product-definition#part",
        Some("test:model:occurrence#root"),
    );
    earlier.name = Some("Earlier".into());
    ir.model.occurrences = vec![root, later, earlier];
    let mut output = Vec::new();
    write_step(
        &ir,
        &mut output,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .expect("write assembly");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode assembly");
    let ordinal = |name: &str| {
        decoded
            .ir()
            .model
            .occurrences
            .iter()
            .find(|occurrence| occurrence.name.as_deref() == Some(name))
            .expect("named occurrence")
            .ordinal
    };
    assert_eq!(ordinal("Earlier"), 0);
    assert_eq!(ordinal("Later"), 1);
}

#[test]
fn occurrence_style_without_a_writable_carrier_is_reported() {
    use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
    use cadmpeg_ir::topology::Color;

    let mut ir = unit_cube().expect("cube");
    ir.model.product_definitions = vec![
        unnamed_definition("test:model:product-definition#assembly", Vec::new()),
        unnamed_definition(
            "test:model:product-definition#part",
            vec![ir.model.bodies[0].id.clone()],
        ),
    ];
    let root = unnamed_occurrence(
        "test:model:occurrence#root",
        "test:model:product-definition#assembly",
        None,
    );
    let child = unnamed_occurrence(
        "test:model:occurrence#child",
        "test:model:product-definition#part",
        Some("test:model:occurrence#root"),
    );
    let appearance = "test:model:appearance#material".try_into().unwrap();
    ir.model.appearances.push(Appearance {
        id: appearance,
        native_ref: None,
        name: None,
        asset_guid: None,
        library_id: None,
        visual_guid: None,
        physical_token: None,
        schema: None,
        category: None,
        base_color: Some(Color::new(0.25, 0.5, 0.75, 1.0).unwrap()),
        properties: std::collections::BTreeMap::default(),
        textures: Vec::new(),
    });
    ir.model.appearance_bindings.push(AppearanceBinding {
        id: "test:model:binding#occurrence".try_into().unwrap(),
        native_ref: None,
        target: AppearanceTarget::Occurrence(child.id.clone()),
        appearance: "test:model:appearance#material".try_into().unwrap(),
        source_entity_id: None,
        object_type: None,
        visible: None,
        channels: std::collections::BTreeMap::default(),
    });
    ir.model.occurrences = vec![root, child];
    let mut output = Vec::new();
    let report = write_step(
        &ir,
        &mut output,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .expect("write assembly");
    assert!(report.losses.iter().any(|loss| {
        loss.code == crate::loss::StepLossCode::DisplayColorUnstyled.kind()
            && loss.message.contains("test:model:binding#occurrence")
    }));
    let text = String::from_utf8(output).unwrap();
    let usage = text
        .lines()
        .find(|line| line.contains("NEXT_ASSEMBLY_USAGE_OCCURRENCE('test:model:occurrence#child'"))
        .unwrap()
        .split_once(" = ")
        .unwrap()
        .0;
    assert!(!text
        .lines()
        .any(|line| line.contains("STYLED_ITEM(") && line.contains(&format!(",{usage})"))));
}

#[test]
fn occurrence_scalar_properties_reference_the_assembly_usage() {
    let mut ir = unit_cube().expect("cube");
    ir.model.product_definitions = vec![
        unnamed_definition("test:model:product-definition#assembly", Vec::new()),
        unnamed_definition(
            "test:model:product-definition#part",
            vec![ir.model.bodies[0].id.clone()],
        ),
    ];
    let root = unnamed_occurrence(
        "test:model:occurrence#root",
        "test:model:product-definition#assembly",
        None,
    );
    let child = unnamed_occurrence(
        "test:model:occurrence#child",
        "test:model:product-definition#part",
        Some("test:model:occurrence#root"),
    );
    ir.model.attributes.push(SourceAttribute {
        id: "test:model:attribute#usage"
            .try_into()
            .expect("attribute id"),
        native_ref: None,
        target: AttributeTarget::Occurrence(child.id.clone()),
        name: "Batch".into(),
        values: vec![Some(AttributeValue::String("B42".into()))],
    });
    ir.model.occurrences = vec![root, child];
    let mut bytes = Vec::new();
    let report = write_step(
        &ir,
        &mut bytes,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .expect("write assembly");
    let text = String::from_utf8(bytes).expect("STEP text");
    let usage = text
        .lines()
        .find(|line| line.contains("NEXT_ASSEMBLY_USAGE_OCCURRENCE('test:model:occurrence#child'"))
        .expect("child usage")
        .split_once(" = ")
        .expect("instance")
        .0;
    let property = text
        .lines()
        .find(|line| line.contains(&format!("PROPERTY_DEFINITION('Batch','',{usage})")))
        .expect("property owns child usage");
    let property_id = property.split_once(" = ").expect("instance").0;
    let link = text
        .lines()
        .find(|line| {
            line.contains(&format!(
                "PROPERTY_DEFINITION_REPRESENTATION({property_id},"
            ))
        })
        .expect("property representation");
    let representation = link
        .split_once(',')
        .expect("representation reference")
        .1
        .trim_end_matches(");");
    let representation_line = text
        .lines()
        .find(|line| line.starts_with(&format!("{representation} = REPRESENTATION(")))
        .expect("value representation");
    let item = representation_line
        .split_once("(#")
        .expect("value item")
        .1
        .split(')')
        .next()
        .expect("item id");
    assert!(text.contains(&format!(
        "#{item} = DESCRIPTIVE_REPRESENTATION_ITEM('','B42')"
    )));
    assert!(!report
        .losses
        .iter()
        .any(|loss| loss.message.contains("test:model:attribute#usage")));
}

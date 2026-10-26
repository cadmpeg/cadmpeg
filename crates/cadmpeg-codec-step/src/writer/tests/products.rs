// SPDX-License-Identifier: Apache-2.0
//! Product and occurrence names and root placements in STEP export.

use std::io::Cursor;

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
        transform: cadmpeg_ir::transform::Transform::identity(),
        linked_prototype: None,
        scale: [cadmpeg_ir::scalar::FiniteReal::ONE; 3],
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
    assert_eq!(record_names(&step, "PRODUCT"), ["Bracket model"]);
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
    assert_eq!(record_names(&step, "PRODUCT"), [""]);

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
    assert_eq!(record_names(&step, "PRODUCT"), ["P-1001"]);
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
            let mut world = occurrence.effective_transform().expect("rigid occurrence");
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
    moved.transform = cadmpeg_ir::transform::Transform::affine([
        [1.0, 0.0, 0.0, 10.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ])
    .expect("finite translation");
    let mut scaled = unnamed_occurrence(
        "test:model:occurrence#scaled",
        "test:model:product-definition#part-0",
        None,
    );
    scaled.scale = [2.0, 1.0, 1.0].map(|value| cadmpeg_ir::scalar::FiniteReal::new(value).unwrap());
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
    local.transform = translation;
    let mut external = unnamed_occurrence(
        "test:model:occurrence#external",
        "test:model:product-definition#part-0",
        None,
    );
    external.prototype = cadmpeg_ir::products::PrototypeReference::External {
        document: cadmpeg_ir::products::ExternalDocument::path("other.FCStd"),
        object: Some("Box".into()),
    };
    external.transform = translation;
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

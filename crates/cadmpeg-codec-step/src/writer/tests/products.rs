// SPDX-License-Identifier: Apache-2.0
//! Product and occurrence names in STEP export.

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

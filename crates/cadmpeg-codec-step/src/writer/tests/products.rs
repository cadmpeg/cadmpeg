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

fn named_root_occurrence(
    id: &str,
    definition: &str,
    name: &str,
) -> cadmpeg_ir::products::Occurrence {
    let mut occurrence = unnamed_occurrence(id, definition, None);
    occurrence.name = Some(name.into());
    occurrence
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

fn assert_synthetic_root_children(ir: &CadIr, expected: &[(&str, &str)]) {
    use cadmpeg_ir::products::{OccurrenceParent, PrototypeReference};

    let roots = ir
        .model
        .occurrences
        .iter()
        .filter(|occurrence| matches!(&occurrence.parent, OccurrenceParent::Root {}))
        .collect::<Vec<_>>();
    assert_eq!(roots.len(), 1, "decoded synthetic root occurrence");
    let root = roots[0];
    let children = ir
        .model
        .occurrences
        .iter()
        .filter(|occurrence| {
            matches!(
                &occurrence.parent,
                OccurrenceParent::Occurrence { occurrence: parent } if parent == &root.id
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(children.len(), expected.len());

    for &(name, product_label) in expected {
        let occurrence = children
            .iter()
            .find(|occurrence| occurrence.name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("missing decoded root occurrence {name}"));
        let PrototypeReference::Local { definition } = &occurrence.prototype else {
            panic!("root occurrence {name} has no local product definition");
        };
        let product = ir
            .model
            .product_definitions
            .iter()
            .find(|product| &product.id == definition)
            .unwrap_or_else(|| panic!("root occurrence {name} has an unknown product"));
        assert_eq!(product.label.as_deref(), Some(product_label));
    }
}

#[test]
fn writer_preserves_distinct_identity_root_occurrences() {
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    let body = ir.model.bodies[0].id.clone();
    let mut first = unnamed_definition("test:model:product-definition#first", vec![body]);
    first.label = Some("First part".into());
    let mut second = unnamed_definition("test:model:product-definition#second", Vec::new());
    second.label = Some("Second part".into());
    ir.model.product_definitions = vec![first, second];
    ir.model.occurrences = vec![
        named_root_occurrence(
            "test:model:occurrence#first-root",
            "test:model:product-definition#first",
            "first-root",
        ),
        named_root_occurrence(
            "test:model:occurrence#second-root",
            "test:model:product-definition#second",
            "second-root",
        ),
    ];

    let step = write_named(&ir, "Assembly model");
    assert_eq!(
        record_names(&step, "PRODUCT"),
        ["First part", "Second part", "Assembly model"]
    );
    assert_eq!(
        record_names(&step, "NEXT_ASSEMBLY_USAGE_OCCURRENCE"),
        ["first-root", "second-root"]
    );

    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(step), &DecodeOptions::default())
        .expect("decode distinct identity roots");
    assert_synthetic_root_children(
        decoded.ir(),
        &[("first-root", "First part"), ("second-root", "Second part")],
    );
}

#[test]
fn writer_does_not_add_a_synthetic_root_for_reused_roots_with_children() {
    use cadmpeg_ir::products::{OccurrenceParent, PrototypeReference};

    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    let body = ir.model.bodies[0].id.clone();
    let mut assembly = unnamed_definition("test:model:product-definition#assembly", Vec::new());
    assembly.label = Some("Assembly".into());
    let mut component = unnamed_definition("test:model:product-definition#component", vec![body]);
    component.label = Some("Component".into());
    let mut other = unnamed_definition("test:model:product-definition#other", Vec::new());
    other.label = Some("Other".into());
    ir.model.product_definitions = vec![assembly, component, other];

    let first_root_id = "test:model:occurrence#first-assembly";
    let second_root_id = "test:model:occurrence#second-assembly";
    ir.model.occurrences = vec![
        named_root_occurrence(
            first_root_id,
            "test:model:product-definition#assembly",
            "first-assembly",
        ),
        named_root_occurrence(
            second_root_id,
            "test:model:product-definition#assembly",
            "second-assembly",
        ),
        named_root_occurrence(
            "test:model:occurrence#other-root",
            "test:model:product-definition#other",
            "other-root",
        ),
        {
            let mut child = unnamed_occurrence(
                "test:model:occurrence#first-component",
                "test:model:product-definition#component",
                Some(first_root_id),
            );
            child.name = Some("component".into());
            child
        },
        {
            let mut child = unnamed_occurrence(
                "test:model:occurrence#second-component",
                "test:model:product-definition#component",
                Some(second_root_id),
            );
            child.name = Some("component".into());
            child
        },
    ];

    let step = write_named(&ir, "Assembly model");
    assert_eq!(
        record_names(&step, "PRODUCT"),
        ["Assembly", "Component", "Other"]
    );
    assert_eq!(
        record_names(&step, "NEXT_ASSEMBLY_USAGE_OCCURRENCE"),
        ["component", "component"]
    );

    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(step), &DecodeOptions::default())
        .expect("decode repeated roots with children");
    let decoded_ir = decoded.ir();
    assert_eq!(decoded_ir.model.occurrences.len(), 4);
    let assembly_definition = decoded_ir
        .model
        .product_definitions
        .iter()
        .find(|product| product.label.as_deref() == Some("Assembly"))
        .expect("assembly definition")
        .id
        .clone();
    let assembly_root = decoded_ir
        .model
        .occurrences
        .iter()
        .find(|occurrence| {
            matches!(&occurrence.parent, OccurrenceParent::Root {})
                && matches!(
                    &occurrence.prototype,
                    PrototypeReference::Local { definition }
                        if definition == &assembly_definition
                )
        })
        .expect("existing direct assembly root");
    let children = decoded_ir
        .model
        .occurrences
        .iter()
        .filter(|occurrence| {
            matches!(
                &occurrence.parent,
                OccurrenceParent::Occurrence { occurrence: parent }
                    if parent == &assembly_root.id
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(children.len(), 2);
}

#[test]
fn writer_keeps_distinct_roots_when_reused_root_children_are_unrepresentable() {
    use cadmpeg_ir::products::{OccurrenceParent, PrototypeReference};

    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    let body = ir.model.bodies[0].id.clone();
    let mut assembly = unnamed_definition("test:model:product-definition#assembly", Vec::new());
    assembly.label = Some("Assembly".into());
    let mut component = unnamed_definition("test:model:product-definition#component", vec![body]);
    component.label = Some("Component".into());
    let mut other = unnamed_definition("test:model:product-definition#other", Vec::new());
    other.label = Some("Other".into());
    ir.model.product_definitions = vec![assembly, component, other];

    let first_root_id = "test:model:occurrence#first-assembly";
    let second_root_id = "test:model:occurrence#second-assembly";
    let mut first_component = unnamed_occurrence(
        "test:model:occurrence#first-component",
        "test:model:product-definition#component",
        Some(first_root_id),
    );
    first_component.name = Some("first-component".into());
    first_component.scale = [2.0, 1.0, 1.0]
        .map(|value| cadmpeg_ir::scalar::FiniteReal::new(value).expect("finite scale"));
    let mut second_component = unnamed_occurrence(
        "test:model:occurrence#second-component",
        "test:model:product-definition#component",
        Some(second_root_id),
    );
    second_component.name = Some("second-component".into());
    second_component.scale = [2.0, 1.0, 1.0]
        .map(|value| cadmpeg_ir::scalar::FiniteReal::new(value).expect("finite scale"));
    ir.model.occurrences = vec![
        named_root_occurrence(
            first_root_id,
            "test:model:product-definition#assembly",
            "first-assembly",
        ),
        named_root_occurrence(
            second_root_id,
            "test:model:product-definition#assembly",
            "second-assembly",
        ),
        named_root_occurrence(
            "test:model:occurrence#other-root",
            "test:model:product-definition#other",
            "other-root",
        ),
        first_component,
        second_component,
    ];

    let options = StepWriteOptions {
        product_name: "Assembly model".into(),
        ..StepWriteOptions::default()
    };
    let mut output = Vec::new();
    let report = write_step(&ir, &mut output, StepSchema::Ap242Edition3, &options)
        .expect("write roots with unrepresentable children");
    assert_eq!(
        report
            .losses
            .iter()
            .filter(
                |loss| loss.code == crate::loss::StepLossCode::OccurrencePlacementNotRigid.kind()
            )
            .count(),
        2
    );
    let step = String::from_utf8(output).expect("STEP text");
    assert_eq!(
        record_names(&step, "NEXT_ASSEMBLY_USAGE_OCCURRENCE"),
        ["first-assembly", "second-assembly", "other-root"]
    );

    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(step), &DecodeOptions::default())
        .expect("decode roots with unrepresentable children");
    let decoded_ir = decoded.ir();
    let model_definition = decoded_ir
        .model
        .product_definitions
        .iter()
        .find(|product| product.label.as_deref() == Some("Assembly model"))
        .expect("synthetic model definition")
        .id
        .clone();
    let model_root = decoded_ir
        .model
        .occurrences
        .iter()
        .find(|occurrence| {
            matches!(&occurrence.parent, OccurrenceParent::Root {})
                && matches!(
                    &occurrence.prototype,
                    PrototypeReference::Local { definition }
                        if definition == &model_definition
                )
        })
        .expect("synthetic model root occurrence");
    let root_uses = decoded_ir
        .model
        .occurrences
        .iter()
        .filter(|occurrence| {
            matches!(
                &occurrence.parent,
                OccurrenceParent::Occurrence { occurrence: parent }
                    if parent == &model_root.id
            )
        })
        .map(|occurrence| occurrence.name.as_deref().expect("root usage name"))
        .collect::<Vec<_>>();
    assert_eq!(
        root_uses,
        ["first-assembly", "second-assembly", "other-root"]
    );
}

#[test]
fn writer_keeps_a_single_identity_root_as_the_direct_product() {
    use cadmpeg_ir::products::{OccurrenceParent, PrototypeReference};

    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    let body = ir.model.bodies[0].id.clone();
    let mut part = unnamed_definition("test:model:product-definition#part", vec![body]);
    part.label = Some("Part".into());
    ir.model.product_definitions = vec![part];
    ir.model.occurrences = vec![unnamed_occurrence(
        "test:model:occurrence#root",
        "test:model:product-definition#part",
        None,
    )];

    let step = write_named(&ir, "Assembly model");
    assert_eq!(record_names(&step, "PRODUCT"), ["Part"]);
    assert!(record_names(&step, "NEXT_ASSEMBLY_USAGE_OCCURRENCE").is_empty());

    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(step), &DecodeOptions::default())
        .expect("decode single identity root");
    assert_eq!(decoded.ir().model.occurrences.len(), 1);
    let occurrence = &decoded.ir().model.occurrences[0];
    assert!(matches!(&occurrence.parent, OccurrenceParent::Root {}));
    let PrototypeReference::Local { definition } = &occurrence.prototype else {
        panic!("single root has no local product definition");
    };
    let product = decoded
        .ir()
        .model
        .product_definitions
        .iter()
        .find(|product| &product.id == definition)
        .expect("single root product");
    assert_eq!(product.label.as_deref(), Some("Part"));
}

#[test]
fn writer_keeps_representable_root_usage_when_another_root_is_scaled() {
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    let body = ir.model.bodies[0].id.clone();
    let mut valid = unnamed_definition("test:model:product-definition#valid", vec![body]);
    valid.label = Some("Valid part".into());
    let mut scaled = unnamed_definition("test:model:product-definition#scaled", Vec::new());
    scaled.label = Some("Scaled part".into());
    ir.model.product_definitions = vec![valid, scaled];
    let valid_root = named_root_occurrence(
        "test:model:occurrence#valid-root",
        "test:model:product-definition#valid",
        "valid-root",
    );
    let mut scaled_root = named_root_occurrence(
        "test:model:occurrence#scaled-root",
        "test:model:product-definition#scaled",
        "scaled-root",
    );
    scaled_root.scale = [2.0, 1.0, 1.0]
        .map(|value| cadmpeg_ir::scalar::FiniteReal::new(value).expect("finite scale"));
    ir.model.occurrences = vec![valid_root, scaled_root];

    let options = StepWriteOptions {
        product_name: "Assembly model".into(),
        ..StepWriteOptions::default()
    };
    let mut output = Vec::new();
    let report = write_step(&ir, &mut output, StepSchema::Ap242Edition3, &options)
        .expect("write mixed roots");
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
    let step = String::from_utf8(output).expect("STEP text");
    assert_eq!(
        record_names(&step, "NEXT_ASSEMBLY_USAGE_OCCURRENCE"),
        ["valid-root"]
    );

    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(step), &DecodeOptions::default())
        .expect("decode mixed roots");
    let valid = decoded
        .ir()
        .model
        .occurrences
        .iter()
        .find(|occurrence| occurrence.name.as_deref() == Some("valid-root"))
        .expect("representable root occurrence survives decode");
    assert!(matches!(
        &valid.parent,
        cadmpeg_ir::products::OccurrenceParent::Occurrence { .. }
    ));
    let cadmpeg_ir::products::PrototypeReference::Local { definition } = &valid.prototype else {
        panic!("representable root has no local product definition");
    };
    let product = decoded
        .ir()
        .model
        .product_definitions
        .iter()
        .find(|product| &product.id == definition)
        .expect("representable root product");
    assert_eq!(product.label.as_deref(), Some("Valid part"));
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

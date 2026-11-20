// SPDX-License-Identifier: Apache-2.0
//! Product and occurrence names and placements in STEP export.

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::examples::unit_cube;
use cadmpeg_ir::ids::{OccurrenceId, ProductDefinitionId};
use cadmpeg_ir::products::{ExternalDocument, Occurrence, ProductDefinition, Storage};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::CadIr;
use cadmpeg_test_support::assembly::resolved_transform;

use crate::export::write_step;
use crate::loss::StepLossCode;
use crate::{StepCodec, StepSchema, StepWriteOptions};

fn unnamed_definition(id: &str, bodies: Vec<cadmpeg_ir::ids::BodyId>) -> ProductDefinition {
    let mut definition = ProductDefinition::new(
        ProductDefinitionId::mint(id).expect("identity grammar"),
        Storage::Embedded,
    );
    definition.kind = Some(cadmpeg_ir::products::ProductDefinitionKind::Part);
    definition.bodies = bodies;
    definition
}

/// An unnamed root definition that owns `occurrences`.
fn root_definition(id: &str, occurrences: &[&str]) -> ProductDefinition {
    let mut definition = ProductDefinition::root(
        ProductDefinitionId::mint(id).expect("identity grammar"),
        ExternalDocument::default(),
    );
    definition.occurrences = occurrences
        .iter()
        .map(|occurrence| OccurrenceId::mint(*occurrence).expect("identity grammar"))
        .collect();
    definition
}

fn unnamed_occurrence(id: &str, definition: &str) -> Occurrence {
    Occurrence::new(
        OccurrenceId::mint(id).expect("identity grammar"),
        ProductDefinitionId::mint(definition).expect("identity grammar"),
        Some(Transform::identity()),
    )
}

fn named_occurrence(id: &str, definition: &str, name: &str) -> Occurrence {
    let mut occurrence = unnamed_occurrence(id, definition);
    occurrence.name = Some(name.into());
    occurrence
}

fn scaled() -> Transform {
    Transform::affine([
        [2.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ])
    .expect("finite scale")
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

fn not_rigid_losses(report: &cadmpeg_ir::report::export::ExportReport) -> usize {
    report
        .losses
        .iter()
        .filter(|loss| loss.code == StepLossCode::OccurrencePlacementNotRigid.kind())
        .count()
}

#[test]
fn writer_names_unnamed_products_and_occurrences_distinctly() {
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    ir.geometry.bodies[0].name = Some("Body1".into());
    let body = ir.geometry.bodies[0].id.clone();
    let occurrences = [
        "test:model:occurrence#occ-0",
        "test:model:occurrence#occ-1",
        "test:model:occurrence#occ-2",
    ];
    ir.model.product_definitions = vec![
        root_definition("test:model:product-definition#assembly", &occurrences),
        unnamed_definition("test:model:product-definition#part-0", vec![body]),
        unnamed_definition("test:model:product-definition#part-1", Vec::new()),
    ];
    ir.model.occurrences = vec![
        unnamed_occurrence(occurrences[0], "test:model:product-definition#part-0"),
        unnamed_occurrence(occurrences[1], "test:model:product-definition#part-1"),
        unnamed_occurrence(occurrences[2], "test:model:product-definition#part-0"),
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
    let body = ir.geometry.bodies[0].id.clone();
    ir.model.product_definitions = vec![
        unnamed_definition("test:first:product-definition#part", vec![body]),
        unnamed_definition("test:second:product-definition#part", Vec::new()),
    ];
    let step = write_named(&ir, "Bracket model");
    assert_eq!(record_names(&step, "PRODUCT"), ["", ""]);

    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(step), &DecodeOptions::default())
        .expect("decode unnamed products");
    assert_eq!(
        decoded
            .ir()
            .model
            .product_definitions
            .iter()
            .filter(|product| !product.is_root())
            .count(),
        2
    );
    for product in &decoded.ir().model.product_definitions {
        assert_eq!(product.label, None);
        assert_eq!(product.source_name, None);
    }
}

#[test]
fn writer_names_the_model_product_from_options_on_both_paths() {
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    ir.geometry.bodies[0].name = Some("Body1".into());
    let step = write_named(&ir, "Bracket model");
    assert_eq!(record_names(&step, "PRODUCT"), ["Bracket model"]);

    let mut root = root_definition("test:model:product-definition#part-0", &[]);
    root.bodies = vec![ir.geometry.bodies[0].id.clone()];
    ir.model.product_definitions = vec![root];
    let step = write_named(&ir, "Bracket model");
    assert_eq!(record_names(&step, "PRODUCT"), ["Bracket model"]);
}

#[test]
fn writer_names_a_product_by_its_part_number() {
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    let mut definition = root_definition("test:model:product-definition#part-0", &[]);
    definition.bodies = vec![ir.geometry.bodies[0].id.clone()];
    definition.part_number = Some("P-1001".into());
    ir.model.product_definitions = vec![definition];
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
        .filter(|occurrence| occurrence.prototype == definition)
        .map(|occurrence| {
            let rows = resolved_transform(ir, &occurrence.id)
                .expect("placement composes to the root")
                .rows();
            [rows[0][3], rows[1][3], rows[2][3]]
        })
        .collect::<Vec<_>>();
    translations.sort_by(|left, right| left.partial_cmp(right).expect("finite"));
    translations
}

/// The product labeled `owner` owns exactly the occurrences `expected`
/// names, each placing the product with the given label.
fn assert_children(ir: &CadIr, owner: &str, expected: &[(&str, &str)]) {
    let owner = ir
        .model
        .product_definitions
        .iter()
        .find(|product| product.label.as_deref() == Some(owner))
        .expect("decoded owner definition");
    let children = owner
        .occurrences
        .iter()
        .map(|id| {
            ir.model
                .occurrences
                .iter()
                .find(|occurrence| &occurrence.id == id)
                .expect("owned occurrence exists")
        })
        .collect::<Vec<_>>();
    assert_eq!(
        children
            .iter()
            .map(|occurrence| occurrence.name.as_deref())
            .collect::<Vec<_>>(),
        expected
            .iter()
            .map(|(name, _)| Some(*name))
            .collect::<Vec<_>>()
    );
    for (occurrence, &(name, product_label)) in children.iter().zip(expected) {
        let product = ir
            .model
            .product_definitions
            .iter()
            .find(|product| product.id == occurrence.prototype)
            .unwrap_or_else(|| panic!("root occurrence {name} has an unknown product"));
        assert_eq!(product.label.as_deref(), Some(product_label));
    }
}

#[test]
fn writer_preserves_distinct_occurrences_of_the_root_definition() {
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    let body = ir.geometry.bodies[0].id.clone();
    let mut first = unnamed_definition("test:model:product-definition#first", vec![body]);
    first.label = Some("First part".into());
    let mut second = unnamed_definition("test:model:product-definition#second", Vec::new());
    second.label = Some("Second part".into());
    ir.model.product_definitions = vec![
        first,
        second,
        root_definition(
            "test:model:product-definition#model",
            &[
                "test:model:occurrence#first-root",
                "test:model:occurrence#second-root",
            ],
        ),
    ];
    ir.model.occurrences = vec![
        named_occurrence(
            "test:model:occurrence#first-root",
            "test:model:product-definition#first",
            "first-root",
        ),
        named_occurrence(
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
        .expect("decode distinct occurrences");
    assert_children(
        decoded.ir(),
        "Assembly model",
        &[("first-root", "First part"), ("second-root", "Second part")],
    );
}

/// A sub-assembly placed twice by the root and once more product placed by
/// it; the sub-assembly owns one component.
fn reused_subassembly(component_transform: Option<Transform>) -> CadIr {
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    let body = ir.geometry.bodies[0].id.clone();
    let mut assembly = unnamed_definition("test:model:product-definition#assembly", Vec::new());
    assembly.label = Some("Assembly".into());
    assembly.occurrences =
        vec![OccurrenceId::mint("test:model:occurrence#component").expect("identity grammar")];
    let mut component = unnamed_definition("test:model:product-definition#component", vec![body]);
    component.label = Some("Component".into());
    let mut other = unnamed_definition("test:model:product-definition#other", Vec::new());
    other.label = Some("Other".into());
    let root = root_definition(
        "test:model:product-definition#model",
        &[
            "test:model:occurrence#first-assembly",
            "test:model:occurrence#second-assembly",
            "test:model:occurrence#other-root",
        ],
    );
    ir.model.product_definitions = vec![assembly, component, other, root];
    let mut component = named_occurrence(
        "test:model:occurrence#component",
        "test:model:product-definition#component",
        "component",
    );
    component.transform = component_transform;
    ir.model.occurrences = vec![
        named_occurrence(
            "test:model:occurrence#first-assembly",
            "test:model:product-definition#assembly",
            "first-assembly",
        ),
        named_occurrence(
            "test:model:occurrence#second-assembly",
            "test:model:product-definition#assembly",
            "second-assembly",
        ),
        named_occurrence(
            "test:model:occurrence#other-root",
            "test:model:product-definition#other",
            "other-root",
        ),
        component,
    ];
    ir
}

#[test]
fn writer_states_a_reused_subassembly_child_once() {
    let ir = reused_subassembly(Some(Transform::identity()));
    let step = write_named(&ir, "Assembly model");
    assert_eq!(
        record_names(&step, "PRODUCT"),
        ["Assembly", "Component", "Other", "Assembly model"]
    );
    let mut usages = record_names(&step, "NEXT_ASSEMBLY_USAGE_OCCURRENCE");
    usages.sort_unstable();
    assert_eq!(
        usages,
        [
            "component",
            "first-assembly",
            "other-root",
            "second-assembly"
        ]
    );

    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(step), &DecodeOptions::default())
        .expect("decode reused sub-assembly");
    let decoded_ir = decoded.ir();
    assert_eq!(decoded_ir.model.occurrences.len(), 4);
    assert_children(
        decoded_ir,
        "Assembly model",
        &[
            ("first-assembly", "Assembly"),
            ("second-assembly", "Assembly"),
            ("other-root", "Other"),
        ],
    );
    let assembly = decoded_ir
        .model
        .product_definitions
        .iter()
        .find(|product| product.label.as_deref() == Some("Assembly"))
        .expect("assembly definition");
    let [component] = assembly.occurrences.as_slice() else {
        panic!("the sub-assembly owns one occurrence: {assembly:#?}");
    };
    let component = decoded_ir
        .model
        .occurrences
        .iter()
        .find(|occurrence| &occurrence.id == component)
        .expect("component occurrence");
    assert_eq!(component.name.as_deref(), Some("component"));
}

#[test]
fn writer_drops_a_non_rigid_nested_placement_and_keeps_its_owner_placed() {
    let ir = reused_subassembly(Some(scaled()));
    let options = StepWriteOptions {
        product_name: "Assembly model".into(),
        ..StepWriteOptions::default()
    };
    let mut output = Vec::new();
    let report = write_step(&ir, &mut output, StepSchema::Ap242Edition3, &options)
        .expect("write a non-rigid nested placement");
    assert_eq!(not_rigid_losses(&report), 1);
    let step = String::from_utf8(output).expect("STEP text");
    assert_eq!(
        record_names(&step, "NEXT_ASSEMBLY_USAGE_OCCURRENCE"),
        ["first-assembly", "second-assembly", "other-root"]
    );

    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(step), &DecodeOptions::default())
        .expect("decode a dropped nested placement");
    assert_children(
        decoded.ir(),
        "Assembly model",
        &[
            ("first-assembly", "Assembly"),
            ("second-assembly", "Assembly"),
            ("other-root", "Other"),
        ],
    );
}

#[test]
fn writer_keeps_a_lone_root_definition_as_the_direct_product() {
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    let body = ir.geometry.bodies[0].id.clone();
    let mut part = root_definition("test:model:product-definition#part", &[]);
    part.bodies = vec![body];
    part.label = Some("Part".into());
    ir.model.product_definitions = vec![part];

    let step = write_named(&ir, "Assembly model");
    assert_eq!(record_names(&step, "PRODUCT"), ["Part"]);
    assert!(record_names(&step, "NEXT_ASSEMBLY_USAGE_OCCURRENCE").is_empty());

    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(step), &DecodeOptions::default())
        .expect("decode a lone root definition");
    assert!(decoded.ir().model.occurrences.is_empty());
    let [product] = decoded.ir().model.product_definitions.as_slice() else {
        panic!("one decoded product");
    };
    assert!(product.is_root());
    assert_eq!(product.label.as_deref(), Some("Part"));
}

#[test]
fn writer_keeps_a_rigid_occurrence_when_its_sibling_is_scaled() {
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    let body = ir.geometry.bodies[0].id.clone();
    let mut valid = unnamed_definition("test:model:product-definition#valid", vec![body]);
    valid.label = Some("Valid part".into());
    let mut scaled_part = unnamed_definition("test:model:product-definition#scaled", Vec::new());
    scaled_part.label = Some("Scaled part".into());
    let root = root_definition(
        "test:model:product-definition#model",
        &[
            "test:model:occurrence#valid-root",
            "test:model:occurrence#scaled-root",
        ],
    );
    ir.model.product_definitions = vec![valid, scaled_part, root];
    let valid_root = named_occurrence(
        "test:model:occurrence#valid-root",
        "test:model:product-definition#valid",
        "valid-root",
    );
    let mut scaled_root = named_occurrence(
        "test:model:occurrence#scaled-root",
        "test:model:product-definition#scaled",
        "scaled-root",
    );
    scaled_root.transform = Some(scaled());
    ir.model.occurrences = vec![valid_root, scaled_root];

    let options = StepWriteOptions {
        product_name: "Assembly model".into(),
        ..StepWriteOptions::default()
    };
    let mut output = Vec::new();
    let report = write_step(&ir, &mut output, StepSchema::Ap242Edition3, &options)
        .expect("write mixed placements");
    assert_eq!(not_rigid_losses(&report), 1);
    let step = String::from_utf8(output).expect("STEP text");
    assert_eq!(
        record_names(&step, "NEXT_ASSEMBLY_USAGE_OCCURRENCE"),
        ["valid-root"]
    );

    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(step), &DecodeOptions::default())
        .expect("decode mixed placements");
    assert_children(
        decoded.ir(),
        "Assembly model",
        &[("valid-root", "Valid part")],
    );
}

#[test]
fn writer_places_rigid_occurrences_under_the_model_product() {
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    let body = ir.geometry.bodies[0].id.clone();
    let mut part = unnamed_definition("test:model:product-definition#part-0", vec![body]);
    part.label = Some("Cube".into());
    let root = root_definition(
        "test:model:product-definition#model",
        &[
            "test:model:occurrence#origin",
            "test:model:occurrence#moved",
            "test:model:occurrence#scaled",
        ],
    );
    ir.model.product_definitions = vec![part, root];
    let mut moved = unnamed_occurrence(
        "test:model:occurrence#moved",
        "test:model:product-definition#part-0",
    );
    moved.transform = Some(
        Transform::affine([
            [1.0, 0.0, 0.0, 10.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ])
        .expect("finite translation"),
    );
    let mut scaled_occurrence = unnamed_occurrence(
        "test:model:occurrence#scaled",
        "test:model:product-definition#part-0",
    );
    scaled_occurrence.transform = Some(scaled());
    ir.model.occurrences = vec![
        unnamed_occurrence(
            "test:model:occurrence#origin",
            "test:model:product-definition#part-0",
        ),
        moved,
        scaled_occurrence,
    ];
    let options = StepWriteOptions {
        product_name: "Bracket model".into(),
        ..StepWriteOptions::default()
    };
    let mut output = Vec::new();
    let report = write_step(&ir, &mut output, StepSchema::Ap242Edition3, &options)
        .expect("write placed occurrences");
    let step = String::from_utf8(output).expect("STEP text");
    assert_eq!(record_names(&step, "PRODUCT"), ["Cube", "Bracket model"]);
    assert_eq!(not_rigid_losses(&report), 1);

    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(step), &DecodeOptions::default())
        .expect("decode placed occurrences");
    assert_eq!(
        world_translations(decoded.ir(), "Cube"),
        [[0.0, 0.0, 0.0], [10.0, 0.0, 0.0]]
    );
}

#[test]
fn writer_reports_a_placed_external_occurrence_only_as_external() {
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    let body = ir.geometry.bodies[0].id.clone();
    let external = ProductDefinition::new(
        ProductDefinitionId::mint("test:model:product-definition#external").expect("identity"),
        Storage::External(ExternalDocument::path("other.FCStd").with_object("Box")),
    );
    ir.model.product_definitions = vec![
        unnamed_definition("test:model:product-definition#part-0", vec![body]),
        external,
        root_definition(
            "test:model:product-definition#model",
            &[
                "test:model:occurrence#local",
                "test:model:occurrence#external",
            ],
        ),
    ];
    let translation = Some(
        Transform::affine([
            [1.0, 0.0, 0.0, 10.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ])
        .expect("finite translation"),
    );
    let mut local = unnamed_occurrence(
        "test:model:occurrence#local",
        "test:model:product-definition#part-0",
    );
    local.transform = translation;
    let mut external = unnamed_occurrence(
        "test:model:occurrence#external",
        "test:model:product-definition#external",
    );
    external.transform = translation;
    ir.model.occurrences = vec![local, external];

    let report = write_step(
        &ir,
        &mut Vec::new(),
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .expect("write external occurrence");
    let reported = |code: StepLossCode| report.losses.iter().any(|loss| loss.code == code.kind());
    assert!(reported(StepLossCode::OccurrenceExternalProduct));
    assert!(!reported(StepLossCode::OccurrenceNoLocalProduct));
    assert!(!reported(StepLossCode::OccurrencePlacementNotRigid));
}

fn translation(x: f64, y: f64, z: f64) -> Transform {
    Transform::affine([[1.0, 0.0, 0.0, x], [0.0, 1.0, 0.0, y], [0.0, 0.0, 1.0, z]])
        .expect("finite translation")
}

fn path(occurrences: &[&str]) -> cadmpeg_ir::design::selection::OccurrencePath {
    cadmpeg_ir::design::selection::OccurrencePath(
        occurrences
            .iter()
            .map(|id| cadmpeg_ir::design::selection::PathStep {
                occurrence: cadmpeg_ir::design::selection::StepRef::Occurrence(
                    OccurrenceId::mint(*id).expect("identity grammar"),
                ),
                configuration: None,
            })
            .collect(),
    )
}

/// The reused sub-assembly with the assemblies at `(10, 0, 0)` and
/// `(0, 20, 0)` and the component at `(1, 2, 3)` in its assembly.
fn placed_subassembly() -> CadIr {
    let mut ir = reused_subassembly(Some(translation(1.0, 2.0, 3.0)));
    for (occurrence, transform) in [
        (
            "test:model:occurrence#first-assembly",
            translation(10.0, 0.0, 0.0),
        ),
        (
            "test:model:occurrence#second-assembly",
            translation(0.0, 20.0, 0.0),
        ),
    ] {
        ir.model
            .occurrences
            .iter_mut()
            .find(|row| row.id.as_str() == occurrence)
            .expect("assembly occurrence")
            .transform = Some(transform);
    }
    ir
}

/// Writes `ir`, decodes the file, and returns the export report with the
/// world translation of every occurrence whose prototype owns bodies.
fn written_body_translations(
    ir: &CadIr,
) -> (cadmpeg_ir::report::export::ExportReport, Vec<[f64; 3]>) {
    let options = StepWriteOptions {
        product_name: "Assembly model".into(),
        ..StepWriteOptions::default()
    };
    let mut output = Vec::new();
    let report =
        write_step(ir, &mut output, StepSchema::Ap242Edition3, &options).expect("write instances");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode instances");
    let decoded = decoded.ir();
    let mut translations = decoded
        .model
        .occurrences
        .iter()
        .filter(|occurrence| {
            decoded
                .model
                .product_definitions
                .iter()
                .any(|product| product.id == occurrence.prototype && !product.bodies.is_empty())
        })
        .map(|occurrence| {
            let rows = resolved_transform(decoded, &occurrence.id)
                .expect("placement composes to the root")
                .rows();
            [rows[0][3], rows[1][3], rows[2][3]]
        })
        .collect::<Vec<_>>();
    translations.sort_by(|left, right| left.partial_cmp(right).expect("finite"));
    (report, translations)
}

fn losses(report: &cadmpeg_ir::report::export::ExportReport, code: StepLossCode) -> usize {
    report
        .losses
        .iter()
        .filter(|loss| loss.code == code.kind())
        .count()
}

/// The root states the component's body for the component inside the
/// second assembly alone, so that body appears once, at
/// `(0, 20, 0) + (1, 2, 3)`.
#[test]
fn writer_places_instance_bodies_in_a_definition_of_their_own() {
    let mut ir = placed_subassembly();
    let body = ir.geometry.bodies[0].id.clone();
    for product in &mut ir.model.product_definitions {
        if product.id.as_str() == "test:model:product-definition#component" {
            product.bodies.clear();
        }
        if product.id.as_str() == "test:model:product-definition#model" {
            product
                .instance_bodies
                .push(cadmpeg_ir::products::InstanceBodies {
                    occurrence: path(&[
                        "test:model:occurrence#second-assembly",
                        "test:model:occurrence#component",
                    ]),
                    bodies: vec![body.clone()],
                });
        }
    }
    let (report, translations) = written_body_translations(&ir);
    assert_eq!(translations, [[1.0, 22.0, 3.0]]);
    assert_eq!(losses(&report, StepLossCode::InstanceDefinitionSplit), 2);
    assert_eq!(
        losses(&report, StepLossCode::BodyWithoutProductRepresentation),
        0
    );
}

/// The second assembly moves its component to `(5, 0, 0)`; the first keeps
/// it at `(1, 2, 3)`.
#[test]
fn writer_places_a_nested_override_in_a_definition_of_its_own() {
    let mut ir = placed_subassembly();
    ir.model
        .occurrences
        .iter_mut()
        .find(|row| row.id.as_str() == "test:model:occurrence#second-assembly")
        .expect("second assembly")
        .overrides
        .push(cadmpeg_ir::products::NestedOverride {
            path: path(&["test:model:occurrence#component"]),
            transform: Some(translation(5.0, 0.0, 0.0)),
            suppressed: None,
            visible: None,
            representation: None,
        });
    let (report, translations) = written_body_translations(&ir);
    assert_eq!(translations, [[5.0, 20.0, 0.0], [11.0, 2.0, 3.0]]);
    assert_eq!(losses(&report, StepLossCode::InstanceDefinitionSplit), 1);
    assert_eq!(losses(&report, StepLossCode::OccurrenceMetadataOmitted), 0);
}

/// The path names the component below the other product, which owns no
/// occurrence.
#[test]
fn writer_reports_an_instance_path_that_names_no_owned_occurrence() {
    let mut ir = placed_subassembly();
    let body = ir.geometry.bodies[0].id.clone();
    ir.model
        .product_definitions
        .iter_mut()
        .find(|product| product.id.as_str() == "test:model:product-definition#model")
        .expect("root definition")
        .instance_bodies
        .push(cadmpeg_ir::products::InstanceBodies {
            occurrence: path(&[
                "test:model:occurrence#other-root",
                "test:model:occurrence#component",
            ]),
            bodies: vec![body],
        });
    let mut output = Vec::new();
    let report = write_step(
        &ir,
        &mut output,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .expect("write an unresolved instance path");
    assert_eq!(losses(&report, StepLossCode::InstancePathUnresolved), 1);
    assert_eq!(losses(&report, StepLossCode::InstanceDefinitionSplit), 0);
}

/// The root places a sub-assembly once at `(10, 0, 0)`; the sub-assembly
/// places its component, which owns the cube, once at `(1, 2, 3)`.
fn single_use_subassembly() -> CadIr {
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    let body = ir.geometry.bodies[0].id.clone();
    let mut assembly = unnamed_definition("test:model:product-definition#assembly", Vec::new());
    assembly.label = Some("Assembly".into());
    assembly.occurrences =
        vec![OccurrenceId::mint("test:model:occurrence#component").expect("identity grammar")];
    let mut component = unnamed_definition("test:model:product-definition#component", vec![body]);
    component.label = Some("Component".into());
    let root = root_definition(
        "test:model:product-definition#model",
        &["test:model:occurrence#assembly"],
    );
    ir.model.product_definitions = vec![assembly, component, root];
    let mut assembly = named_occurrence(
        "test:model:occurrence#assembly",
        "test:model:product-definition#assembly",
        "assembly",
    );
    assembly.transform = Some(translation(10.0, 0.0, 0.0));
    let mut component = named_occurrence(
        "test:model:occurrence#component",
        "test:model:product-definition#component",
        "component",
    );
    component.transform = Some(translation(1.0, 2.0, 3.0));
    ir.model.occurrences = vec![assembly, component];
    ir
}

fn with_instance_bodies(mut ir: CadIr, bodies: Vec<cadmpeg_ir::ids::BodyId>) -> CadIr {
    ir.model
        .product_definitions
        .iter_mut()
        .find(|product| product.id.as_str() == "test:model:product-definition#model")
        .expect("root definition")
        .instance_bodies
        .push(cadmpeg_ir::products::InstanceBodies {
            occurrence: path(&[
                "test:model:occurrence#assembly",
                "test:model:occurrence#component",
            ]),
            bodies,
        });
    ir
}

/// Writes `ir` and decodes the file.
fn written_and_read(ir: &CadIr) -> (cadmpeg_ir::report::export::ExportReport, CadIr) {
    let mut output = Vec::new();
    let report = write_step(
        ir,
        &mut output,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .expect("write instances");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode instances");
    (report, decoded.ir().clone())
}

/// The decoded document has one root, the model, and places the
/// sub-assembly and its component once each.
fn assert_single_use_structure(decoded: &CadIr) {
    let roots = decoded
        .model
        .product_definitions
        .iter()
        .filter(|product| {
            !decoded
                .model
                .occurrences
                .iter()
                .any(|occurrence| occurrence.prototype == product.id)
        })
        .collect::<Vec<_>>();
    assert_eq!(roots.len(), 1, "one root: {roots:#?}");
    assert!(
        roots[0].occurrences.len() == 1,
        "the root places only the sub-assembly: {:#?}",
        roots[0]
    );
    assert_eq!(decoded.model.occurrences.len(), 2);
    assert_eq!(decoded.model.product_definitions.len(), 3);
}

/// The component's only instance shows the component's own body, stated by
/// the root two steps down; nothing else places the sub-assembly or the
/// component, so neither is written beside its split instance.
#[test]
fn writer_does_not_leave_a_once_placed_prototype_beside_its_split_instance() {
    let ir = single_use_subassembly();
    let body = ir.geometry.bodies[0].id.clone();
    let ir = with_instance_bodies(ir, vec![body]);
    let (report, translations) = written_body_translations(&ir);
    assert_eq!(translations, [[11.0, 2.0, 3.0]]);
    assert_eq!(losses(&report, StepLossCode::InstanceDefinitionSplit), 2);
    assert_eq!(
        losses(&report, StepLossCode::InstancePrototypeBodiesOmitted),
        0
    );
    assert_eq!(
        losses(&report, StepLossCode::BodyWithoutProductRepresentation),
        0
    );
    let (_, decoded) = written_and_read(&ir);
    assert_single_use_structure(&decoded);
    assert_eq!(decoded.geometry.bodies.len(), 1);
}

/// The component's only instance shows no body, so the component's body is
/// shown nowhere: it is charged, and not written at the origin.
#[test]
fn writer_charges_the_body_of_a_prototype_no_instance_shows() {
    let ir = with_instance_bodies(single_use_subassembly(), Vec::new());
    let (report, decoded) = written_and_read(&ir);
    assert_eq!(
        losses(&report, StepLossCode::InstancePrototypeBodiesOmitted),
        1
    );
    assert_eq!(
        losses(&report, StepLossCode::BodyWithoutProductRepresentation),
        0
    );
    assert_single_use_structure(&decoded);
    assert!(
        decoded
            .model
            .product_definitions
            .iter()
            .all(|product| product.bodies.is_empty()),
        "no definition shows a body: {:#?}",
        decoded.model.product_definitions
    );
    assert!(decoded.geometry.bodies.is_empty());
}

/// The root moves the component of its only sub-assembly to `(5, 0, 0)`.
#[test]
fn writer_does_not_leave_a_once_placed_prototype_beside_its_nested_override() {
    let mut ir = single_use_subassembly();
    ir.model
        .occurrences
        .iter_mut()
        .find(|row| row.id.as_str() == "test:model:occurrence#assembly")
        .expect("assembly occurrence")
        .overrides
        .push(cadmpeg_ir::products::NestedOverride {
            path: path(&["test:model:occurrence#component"]),
            transform: Some(translation(5.0, 0.0, 0.0)),
            suppressed: None,
            visible: None,
            representation: None,
        });
    let (report, translations) = written_body_translations(&ir);
    assert_eq!(translations, [[15.0, 0.0, 0.0]]);
    assert_eq!(losses(&report, StepLossCode::InstanceDefinitionSplit), 1);
    assert_eq!(
        losses(&report, StepLossCode::InstancePrototypeBodiesOmitted),
        0
    );
    let (_, decoded) = written_and_read(&ir);
    assert_single_use_structure(&decoded);
    assert_eq!(decoded.geometry.bodies.len(), 1);
}

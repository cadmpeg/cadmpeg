// SPDX-License-Identifier: Apache-2.0
//! Encode sets (§3.4): one output per stored definition.

use crate::codec::write::test_support::CATALOG_WRITE_TARGETS;
use crate::codec::write::{
    target::{Catalog, DialectFree, ResolvedWrite},
    ArenaCoverage, ArenaDisposition, ArenaDispositions, EncodeError, EncodeInput, Encoder,
    EncoderBackend, ExportBody, OutputNames, Outputs, PlannedOutput,
};
use crate::codec::FormatId;
use crate::examples::unit_cube;
use crate::report::strictness::code_name;
use crate::CadIr;
use cadmpeg_core::serde_erase::json;

use super::{uniform_dispositions, NeutralEncoder};
use cadmpeg_core::decode::{DecodeContext, DecodeDepth};
use cadmpeg_core::CodecError;

fn rooted(id: &str) -> (CadIr, crate::ids::ProductDefinitionId) {
    let mut ir = CadIr::empty();
    let definition =
        crate::ids::ProductDefinitionId::try_from(format!("test:model:definition#{id}")).unwrap();
    ir.model
        .product_definitions
        .push(crate::products::ProductDefinition::root(
            definition.clone(),
            crate::products::ExternalDocument::default(),
        ));
    (ir, definition)
}

fn output_name(name: &str) -> cadmpeg_core::text::NonBlankString {
    cadmpeg_core::text::NonBlankString::new(name).unwrap()
}

#[test]
fn an_encode_whose_outputs_lack_a_name_for_an_output_is_refused_before_planning() {
    let (ir, root) = rooted("root");
    let names = OutputNames::new();
    let error = NeutralEncoder
        .plan(EncodeInput::new(&ir, None).with_outputs(Outputs::Named(&names)))
        .unwrap_err();
    let EncodeError::OutputUnnamed { definition } = error else {
        panic!("an unnamed output refuses the encode, got {error}")
    };
    assert_eq!(definition, root);
}

#[test]
fn the_plan_lists_the_outputs_it_writes_with_the_callers_names() {
    let (ir, root) = rooted("root");
    let names = OutputNames::from([(root.clone(), output_name("part.out"))]);
    let plan = NeutralEncoder
        .plan(EncodeInput::new(&ir, None).with_outputs(Outputs::Named(&names)))
        .unwrap();
    assert_eq!(
        plan.outputs(),
        [PlannedOutput {
            definition: Some(root),
            name: Some(output_name("part.out")),
        }]
    );
}

#[test]
fn the_destination_names_the_root_output_of_an_encode_that_names_none() {
    let (ir, root) = rooted("root");
    let plan = NeutralEncoder.plan(EncodeInput::new(&ir, None)).unwrap();
    assert_eq!(
        plan.outputs(),
        [PlannedOutput {
            definition: Some(root),
            name: None,
        }]
    );
}

/// A writer of a format that stores one definition per file. Its payload
/// names the root definition, the bodies, and the link paths it receives.
struct PerFileEncoder;

impl EncoderBackend for PerFileEncoder {
    const FORMAT: FormatId = FormatId::new("cadir");
    const NAME: &'static str = "CADIR";
    const REQUEST: DecodeDepth = DecodeDepth::Full;
    const REUSES_SOURCE: bool = false;
    const DEFINITION_PER_FILE: bool = true;
    type Target = DialectFree;
    const TARGET: DialectFree = DialectFree;

    fn plan_resolved(
        &self,
        _: &DecodeContext<'_>,
        input: EncodeInput<'_>,
        (): (),
    ) -> Result<ExportBody, CodecError> {
        let definitions = &input.ir.model.product_definitions;
        let root = definitions
            .iter()
            .find(|definition| definition.is_root())
            .map_or("", |definition| definition.id.as_str());
        let bodies = input
            .ir
            .geometry
            .bodies
            .iter()
            .map(|body| body.id.as_str())
            .collect::<Vec<_>>();
        let links = definitions
            .iter()
            .filter_map(|definition| match &definition.storage {
                crate::products::Storage::External(document) => document
                    .path
                    .as_ref()
                    .map(cadmpeg_core::text::NonBlankString::as_str),
                _ => None,
            })
            .collect::<Vec<_>>();
        let payload = format!("{root}|{}|{}", bodies.join(","), links.join(","));
        Ok(ExportBody::synthesized(
            payload.into_bytes(),
            input.ir,
            ArenaCoverage::Declared(ArenaDispositions {
                product_definitions: ArenaDisposition::Omitted,
                ..uniform_dispositions(ArenaDisposition::Written)
            }),
            crate::codec::write::NativeDispositions::written(),
        ))
    }
}

fn definition_id(key: &str) -> crate::ids::ProductDefinitionId {
    crate::ids::ProductDefinitionId::try_from(format!("test:model:definition#{key}")).unwrap()
}

fn bare_body(key: &str) -> crate::topology::Body {
    crate::topology::Body {
        id: crate::ids::BodyId::try_from(format!("test:model:body#{key}")).unwrap(),
        kind: crate::topology::BodyKind::Solid,
        transform: None,
        name: None,
        color: None,
        visible: None,
        unstated: None,
        native_ref: None,
    }
}

/// A root definition holding the unit cube and placing a part stored in a
/// file of its own, which holds one body, and an embedded definition that no
/// occurrence places, which holds another.
fn two_file_document() -> CadIr {
    use crate::products::{ExternalDocument, Occurrence, ProductDefinition, Storage};
    let mut ir = unit_cube().unwrap();
    let cube = ir.geometry.bodies[0].id.clone();
    let part_body = bare_body("part");
    let loose_body = bare_body("loose");
    let mut root = ProductDefinition::root(definition_id("root"), ExternalDocument::default());
    root.bodies.push(cube);
    let occurrence =
        crate::ids::OccurrenceId::try_from("test:model:occurrence#part".to_owned()).unwrap();
    root.occurrences.push(occurrence.clone());
    let mut part = ProductDefinition::new(
        definition_id("part"),
        Storage::File {
            document: ExternalDocument::path("part.source"),
            source: json::from_value(cadmpeg_core::json!({
                "identity": {"kind": "unclassified", "format": "step"},
                "attributes": {},
            }))
            .unwrap(),
        },
    );
    part.bodies.push(part_body.id.clone());
    let mut loose = ProductDefinition::new(definition_id("loose"), Storage::Embedded);
    loose.bodies.push(loose_body.id.clone());
    ir.model.product_definitions = vec![root, part, loose];
    ir.model
        .occurrences
        .push(Occurrence::new(occurrence, definition_id("part"), None));
    ir.geometry.bodies.extend([part_body, loose_body]);
    ir
}

#[test]
fn a_per_file_encode_writes_one_output_per_file_definition() {
    let ir = two_file_document();
    let names = OutputNames::from([
        (definition_id("root"), output_name("assembly.out")),
        (definition_id("part"), output_name("part.out")),
    ]);
    assert_eq!(
        PerFileEncoder.output_definitions(&ir),
        [Some(definition_id("root")), Some(definition_id("part"))]
    );
    let plan = PerFileEncoder
        .plan(EncodeInput::new(&ir, None).with_outputs(Outputs::Named(&names)))
        .unwrap();
    assert_eq!(
        plan.outputs(),
        [
            PlannedOutput {
                definition: Some(definition_id("root")),
                name: Some(output_name("assembly.out")),
            },
            PlannedOutput {
                definition: Some(definition_id("part")),
                name: Some(output_name("part.out")),
            },
        ]
    );
    let cube = ir.geometry.bodies[0].id.as_str().to_owned();
    let mut written = Vec::new();
    let report = plan
        .write_each(|output, bytes| {
            written.push((
                output.name.clone(),
                String::from_utf8_lossy(bytes).into_owned(),
            ));
            Ok(())
        })
        .unwrap();
    assert_eq!(
        written,
        [
            (
                Some(output_name("assembly.out")),
                format!("test:model:definition#root|{cube},test:model:body#loose|part.out"),
            ),
            (
                Some(output_name("part.out")),
                "test:model:definition#part|test:model:body#part|".to_owned(),
            ),
        ]
    );
    let placed = report
        .losses
        .iter()
        .filter(|loss| code_name(&loss.code) == "export:definition.placed")
        .collect::<Vec<_>>();
    assert_eq!(placed.len(), 1, "{:?}", report.losses);
    assert!(placed[0].message.contains("test:model:definition#loose"));
    assert!(!placed[0].message.contains("test:model:definition#part"));
}

#[test]
fn an_unplaced_definition_with_occurrences_or_meshes_is_named_as_placed() {
    use crate::math::Point3;
    use crate::products::{Occurrence, ProductDefinition, Storage};
    use crate::tessellation::{Meshed, Tessellation, TessellationMesh};
    let mut ir = two_file_document();
    let leaf_body = bare_body("leaf");
    let mut leaf = ProductDefinition::new(definition_id("leaf"), Storage::Embedded);
    leaf.bodies.push(leaf_body.id.clone());
    let occurrence =
        crate::ids::OccurrenceId::try_from("test:model:occurrence#leaf".to_owned()).unwrap();
    let mut group = ProductDefinition::new(definition_id("group"), Storage::Embedded);
    group.occurrences.push(occurrence.clone());
    let meshed = ProductDefinition::new(definition_id("meshed"), Storage::Embedded);
    let empty = ProductDefinition::new(definition_id("empty"), Storage::Embedded);
    ir.model
        .product_definitions
        .extend([leaf, group, meshed, empty]);
    ir.model
        .occurrences
        .push(Occurrence::new(occurrence, definition_id("leaf"), None));
    ir.geometry.bodies.push(leaf_body);
    let mut mesh = Tessellation::new(
        "test:model:tessellation#meshed".try_into().unwrap(),
        TessellationMesh::List {
            vertices: vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(0.0, 1.0, 0.0),
            ],
            triangles: vec![[0, 1, 2]],
        },
        Vec::new(),
    )
    .unwrap();
    mesh.of = Meshed::Display {
        definition: definition_id("meshed"),
    };
    ir.model.tessellations.push(mesh);
    let named = |coverage: ArenaDisposition| {
        let coverage = ArenaCoverage::Declared(ArenaDispositions {
            product_definitions: ArenaDisposition::Omitted,
            tessellations: coverage,
            ..uniform_dispositions(ArenaDisposition::Written)
        });
        let loss = crate::codec::write::placed_definition_loss(&ir, &coverage, "test").unwrap();
        ["loose", "group", "meshed", "leaf", "part", "empty"]
            .into_iter()
            .filter(|key| loss.message.contains(definition_id(key).as_str()))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        named(ArenaDisposition::Written),
        ["loose", "group", "meshed"]
    );
    assert_eq!(named(ArenaDisposition::Omitted), ["loose", "group"]);
}

#[test]
fn a_per_file_encode_that_names_only_the_root_is_refused_before_planning() {
    let ir = two_file_document();
    let error = PerFileEncoder
        .plan(EncodeInput::new(&ir, None))
        .unwrap_err();
    let EncodeError::OutputUnnamed { definition } = error else {
        panic!("the part's output has no name, got {error}")
    };
    assert_eq!(definition, definition_id("part"));
}

#[test]
fn a_plan_with_several_outputs_refuses_one_destination() {
    let ir = two_file_document();
    let names = OutputNames::from([
        (definition_id("root"), output_name("assembly.out")),
        (definition_id("part"), output_name("part.out")),
    ]);
    let plan = PerFileEncoder
        .plan(EncodeInput::new(&ir, None).with_outputs(Outputs::Named(&names)))
        .unwrap();
    let mut bytes = Vec::new();
    let error = plan.write_to(&mut bytes).unwrap_err();
    let EncodeError::OutputUnnamed { definition } = error else {
        panic!("the destination names no output of the part, got {error}")
    };
    assert_eq!(definition, definition_id("part"));
    assert!(bytes.is_empty());
}

#[test]
fn two_outputs_with_one_name_are_refused_before_planning() {
    let ir = two_file_document();
    let names = OutputNames::from([
        (definition_id("root"), output_name("one.out")),
        (definition_id("part"), output_name("one.out")),
    ]);
    let error = PerFileEncoder
        .plan(EncodeInput::new(&ir, None).with_outputs(Outputs::Named(&names)))
        .unwrap_err();
    let EncodeError::OutputUnnamed { definition } = error else {
        panic!("the part's output takes the root's name, got {error}")
    };
    assert_eq!(definition, definition_id("part"));
}

/// A writer of a format that stores one definition per file. Its payload
/// names the features and the tessellations of the document it receives,
/// and it refuses the definitions of `refused`.
struct HeldRecordsEncoder {
    refused: &'static [&'static str],
}

impl EncoderBackend for HeldRecordsEncoder {
    const FORMAT: FormatId = FormatId::new("cadir");
    const NAME: &'static str = "CADIR";
    const REQUEST: DecodeDepth = DecodeDepth::Full;
    const REUSES_SOURCE: bool = false;
    const DEFINITION_PER_FILE: bool = true;
    type Target = DialectFree;
    const TARGET: DialectFree = DialectFree;

    fn plan_resolved(
        &self,
        _: &DecodeContext<'_>,
        input: EncodeInput<'_>,
        (): (),
    ) -> Result<ExportBody, CodecError> {
        let model = &input.ir.model;
        let root = model
            .product_definitions
            .iter()
            .find(|definition| definition.is_root())
            .map_or("", |definition| definition.id.as_str());
        if self.refused.contains(&root) {
            return Err(CodecError::malformed(
                "the test writer refuses this definition",
            ));
        }
        let features = model
            .features
            .iter()
            .map(|feature| feature.id.as_str())
            .collect::<Vec<_>>();
        let tessellations = model
            .tessellations
            .iter()
            .map(|tessellation| tessellation.id.as_str())
            .collect::<Vec<_>>();
        let payload = format!("{}|{}", features.join(","), tessellations.join(","));
        Ok(ExportBody::synthesized(
            payload.into_bytes(),
            input.ir,
            ArenaCoverage::Complete,
            crate::codec::write::NativeDispositions::written(),
        ))
    }
}

/// [`two_file_document`] with one feature of each file definition and one
/// tessellation of each of their bodies.
fn two_file_document_with_records() -> CadIr {
    use crate::features::evaluation::FeatureEvaluation;
    use crate::features::{Feature, FeatureDefinition, FeatureOperation};
    use crate::tessellation::{Meshed, Tessellation, TessellationMesh};
    let mut ir = two_file_document();
    let bodies = [
        ("root", ir.geometry.bodies[0].id.clone()),
        ("part", bare_body("part").id),
    ];
    for (ordinal, (key, body)) in (0..).zip(bodies) {
        ir.model.features.push(Feature {
            product_definition: Some(definition_id(key)),
            id: format!("test:model:feature#{key}").try_into().unwrap(),
            ordinal,
            name: None,
            suppressed: None,
            dependencies: crate::features::selections::DistinctMembers::default(),
            source_properties: std::collections::BTreeMap::default(),
            source_tag: None,
            source_text: None,
            source_content: crate::features::FeatureContent::default(),
            evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
                FeatureOperation::StoredGeometry {},
            )),
            native_ref: None,
        });
        ir.model.tessellations.push(
            Tessellation::new(
                format!("test:model:tessellation#{key}").try_into().unwrap(),
                TessellationMesh::List {
                    vertices: vec![
                        crate::math::Point3::new(0.0, 0.0, 0.0),
                        crate::math::Point3::new(1.0, 0.0, 0.0),
                        crate::math::Point3::new(0.0, 1.0, 0.0),
                    ],
                    triangles: vec![[0, 1, 2]],
                },
                Vec::new(),
            )
            .unwrap()
            .with_of(Meshed::Body {
                body,
                faces: Vec::new(),
            }),
        );
    }
    ir
}

#[test]
fn each_output_of_an_encode_set_holds_only_its_definitions_records() {
    let ir = two_file_document_with_records();
    let names = OutputNames::from([
        (definition_id("root"), output_name("assembly.out")),
        (definition_id("part"), output_name("part.out")),
    ]);
    let plan = HeldRecordsEncoder { refused: &[] }
        .plan(EncodeInput::new(&ir, None).with_outputs(Outputs::Named(&names)))
        .unwrap();
    let mut written = Vec::new();
    plan.write_each(|_, bytes| {
        written.push(String::from_utf8_lossy(bytes).into_owned());
        Ok(())
    })
    .unwrap();
    assert_eq!(
        written,
        [
            "test:model:feature#root|test:model:tessellation#root",
            "test:model:feature#part|test:model:tessellation#part",
        ]
    );
}

#[test]
fn an_output_that_cannot_be_written_leaves_the_others_written() {
    let ir = two_file_document_with_records();
    let names = OutputNames::from([
        (definition_id("root"), output_name("assembly.out")),
        (definition_id("part"), output_name("part.out")),
    ]);
    let input = EncodeInput::new(&ir, None).with_outputs(Outputs::Named(&names));
    let plan = HeldRecordsEncoder {
        refused: &["test:model:definition#part"],
    }
    .plan(input)
    .unwrap();
    assert_eq!(
        plan.outputs(),
        [PlannedOutput {
            definition: Some(definition_id("root")),
            name: Some(output_name("assembly.out")),
        }]
    );
    let unwritten = plan
        .report()
        .losses
        .iter()
        .filter(|loss| code_name(&loss.code) == "export:output.unwritten")
        .collect::<Vec<_>>();
    assert_eq!(unwritten.len(), 1, "{:?}", plan.report().losses);
    assert!(unwritten[0].message.contains("test:model:definition#part"));

    let error = HeldRecordsEncoder {
        refused: &["test:model:definition#root", "test:model:definition#part"],
    }
    .plan(input)
    .unwrap_err();
    assert!(matches!(error, EncodeError::Backend(_)), "got {error:?}");
}

/// A writer of a format that stores one definition per file. It keeps the
/// document of each output it plans.
#[derive(Default)]
struct PlannedDocuments(std::sync::Mutex<Vec<CadIr>>);

impl EncoderBackend for PlannedDocuments {
    const FORMAT: FormatId = FormatId::new("cadir");
    const NAME: &'static str = "CADIR";
    const REQUEST: DecodeDepth = DecodeDepth::Full;
    const REUSES_SOURCE: bool = false;
    const DEFINITION_PER_FILE: bool = true;
    type Target = DialectFree;
    const TARGET: DialectFree = DialectFree;

    fn plan_resolved(
        &self,
        _: &DecodeContext<'_>,
        input: EncodeInput<'_>,
        (): (),
    ) -> Result<ExportBody, CodecError> {
        self.0.lock().unwrap().push(input.ir.clone());
        Ok(ExportBody::synthesized(
            Vec::new(),
            input.ir,
            ArenaCoverage::Complete,
            crate::codec::write::NativeDispositions::written(),
        ))
    }
}

/// [`two_file_document_with_records`] with a feature, a parameter, and a
/// sketch that no definition owns, and a feature that no definition owns
/// and that the part's feature depends on.
fn document_with_unowned_records() -> CadIr {
    let mut ir = two_file_document_with_records();
    let mut loose = ir.model.features[0].clone();
    loose.product_definition = None;
    loose.id = "test:model:feature#loose".try_into().unwrap();
    let mut shared = loose.clone();
    shared.id = "test:model:feature#shared".try_into().unwrap();
    let part = ir
        .model
        .features
        .iter_mut()
        .find(|feature| feature.id.as_str() == "test:model:feature#part")
        .unwrap();
    part.dependencies = std::iter::once(shared.id.clone()).collect();
    ir.model.features.extend([loose, shared]);
    ir.model
        .parameters
        .push(crate::features::parameters::DesignParameter {
            product_definition: None,
            id: crate::features::ParameterId::mint("test:model:parameter#loose").unwrap(),
            owner: None,
            ordinal: 0,
            name: "D1".into(),
            expression: "1mm".into(),
            display: None,
            value: None,
            dependencies: crate::features::selections::DistinctMembers::default(),
            properties: std::collections::BTreeMap::new(),
            pmi: None,
            native_ref: None,
        });
    ir.model.sketches.push(crate::sketches::Sketch {
        id: "test:model:sketch#loose".try_into().unwrap(),
        name: None,
        configuration: None,
        visible: None,
        placement: crate::sketches::SketchPlacement::Unresolved {},
        profiles: crate::sketches::SketchProfiles::default(),
        native_ref: None,
    });
    ir
}

#[test]
fn a_record_no_definition_owns_is_held_by_the_root_output_and_where_it_is_named() {
    let ir = document_with_unowned_records();
    let names = OutputNames::from([
        (definition_id("root"), output_name("assembly.out")),
        (definition_id("part"), output_name("part.out")),
    ]);
    let planned = PlannedDocuments::default();
    let plan = planned
        .plan(EncodeInput::new(&ir, None).with_outputs(Outputs::Named(&names)))
        .unwrap();
    assert_eq!(plan.outputs().len(), 2);
    let documents = planned.0.into_inner().unwrap();
    let held = |document: &CadIr| {
        let model = &document.model;
        model
            .features
            .iter()
            .map(|feature| feature.id.as_str().to_owned())
            .chain(
                model
                    .parameters
                    .iter()
                    .map(|parameter| parameter.id.as_str().to_owned()),
            )
            .chain(
                model
                    .sketches
                    .iter()
                    .map(|sketch| sketch.id.as_str().to_owned()),
            )
            .collect::<std::collections::BTreeSet<_>>()
    };
    assert_eq!(
        held(&documents[0]),
        [
            "test:model:feature#loose",
            "test:model:feature#root",
            "test:model:feature#shared",
            "test:model:parameter#loose",
            "test:model:sketch#loose",
        ]
        .map(str::to_owned)
        .into()
    );
    assert_eq!(
        held(&documents[1]),
        ["test:model:feature#part", "test:model:feature#shared"]
            .map(str::to_owned)
            .into()
    );
}

/// The ids of the records of `arena` in `document`.
fn ids_of(document: &CadIr, arena: &str) -> Vec<String> {
    document
        .arenas()
        .record_ids()
        .into_iter()
        .find(|(name, _)| name.as_str() == arena)
        .map(|(_, (_, ids))| ids.into_iter().map(str::to_owned).collect())
        .unwrap_or_default()
}

/// The messages of the `export:record.unheld` losses of `report`.
fn unheld(report: &crate::report::export::ExportReport) -> Vec<&str> {
    report
        .losses
        .iter()
        .filter(|loss| code_name(&loss.code) == "export:record.unheld")
        .map(|loss| loss.message.as_str())
        .collect()
}

#[test]
fn a_record_that_needs_a_record_of_another_output_is_charged_where_no_output_holds_it() {
    let mut ir = two_file_document_with_records();
    let part = crate::features::FeatureId::try_from("test:model:feature#part".to_owned()).unwrap();
    let mut asm = ir.model.features[0].clone();
    asm.product_definition = None;
    asm.id = "test:model:feature#asm".try_into().unwrap();
    asm.dependencies = std::iter::once(part.clone()).collect();
    let mut rootdep = ir.model.features[0].clone();
    rootdep.id = "test:model:feature#rootdep".try_into().unwrap();
    rootdep.dependencies = std::iter::once(part).collect();
    ir.model.features.extend([asm, rootdep]);
    let names = OutputNames::from([
        (definition_id("root"), output_name("assembly.out")),
        (definition_id("part"), output_name("part.out")),
    ]);
    let planned = PlannedDocuments::default();
    let plan = planned
        .plan(EncodeInput::new(&ir, None).with_outputs(Outputs::Named(&names)))
        .unwrap();
    let documents = planned.0.into_inner().unwrap();
    assert_eq!(
        ids_of(&documents[0], "features"),
        ["test:model:feature#root"]
    );
    assert_eq!(
        ids_of(&documents[1], "features"),
        ["test:model:feature#part"]
    );
    let [message] = unheld(plan.report())[..] else {
        panic!("one record.unheld loss: {:?}", plan.report().losses)
    };
    for entry in [
        r#"test:model:feature#asm from "assembly.out" needs test:model:feature#part of "part.out""#,
        r#"test:model:feature#rootdep from "assembly.out" needs test:model:feature#part of "part.out""#,
    ] {
        assert!(message.contains(entry), "{entry} in {message}");
    }
}

/// [`two_file_document_with_records`] where the part places an embedded
/// pin, with joints of the root file: one grounded on the root, one
/// grounded on the pin, one between the part's occurrence and the pin, one
/// between the root and the pin, and one on the root that a `Joint` feature
/// of the part names; and a joint of the part's file grounded on its root.
fn document_with_joints() -> CadIr {
    use crate::products::{
        AssemblyJoint, JointConnector, JointId, JointOperand, Occurrence, PairedJointKind,
        ProductDefinition, Storage,
    };
    use crate::transform::Transform;
    let mut ir = two_file_document_with_records();
    let pin = crate::ids::OccurrenceId::try_from("test:model:occurrence#pin".to_owned()).unwrap();
    let placed =
        crate::ids::OccurrenceId::try_from("test:model:occurrence#part".to_owned()).unwrap();
    ir.model.product_definitions.push(ProductDefinition::new(
        definition_id("pin"),
        Storage::Embedded,
    ));
    ir.model
        .product_definitions
        .iter_mut()
        .find(|definition| definition.id == definition_id("part"))
        .unwrap()
        .occurrences
        .push(pin.clone());
    ir.model
        .occurrences
        .push(Occurrence::new(pin.clone(), definition_id("pin"), None));
    let connector = |operand| JointConnector {
        operand,
        frame: Transform::identity(),
        detached: false,
    };
    let id = |key: &str| JointId::mint(format!("test:model:joint#{key}")).unwrap();
    ir.model.assembly_joints = vec![
        AssemblyJoint::grounded(
            id("root"),
            connector(JointOperand::root("origin", Vec::new())),
            None,
        ),
        AssemblyJoint::grounded(
            id("pin"),
            connector(JointOperand::occurrence(pin.clone(), "pin", Vec::new())),
            None,
        ),
        AssemblyJoint::paired(
            id("local"),
            PairedJointKind::Fixed {
                angle: None,
                translation_offset: None,
                angular_limits: None,
                linear_limits: None,
            },
            [
                connector(JointOperand::root("origin", Vec::new())),
                connector(JointOperand::occurrence(pin.clone(), "pin", Vec::new())),
            ],
            None,
        ),
        AssemblyJoint::paired(
            id("mixed"),
            PairedJointKind::Fixed {
                angle: None,
                translation_offset: None,
                angular_limits: None,
                linear_limits: None,
            },
            [
                connector(JointOperand::occurrence(placed, "part", Vec::new())),
                connector(JointOperand::occurrence(pin, "pin", Vec::new())),
            ],
            None,
        ),
        AssemblyJoint::grounded(
            id("featured"),
            connector(JointOperand::root("origin", Vec::new())),
            None,
        ),
        AssemblyJoint::grounded(
            JointId::mint("test:part@1:joint#own").unwrap(),
            connector(JointOperand::root("origin", Vec::new())),
            None,
        ),
    ];
    let mut featured = ir.model.features[1].clone();
    featured.id = "test:model:feature#featured".try_into().unwrap();
    featured.evaluation = crate::features::evaluation::FeatureEvaluation::from_definition(
        crate::features::FeatureDefinition::Operation(
            crate::features::FeatureOperation::AssemblyJoint {
                joint: id("featured"),
            },
        ),
    );
    ir.model.features.push(featured);
    ir
}

/// A joint goes with the `Joint` feature that names it, and otherwise to
/// the output of the file it was read from, so that a connector on
/// `Root {}` keeps naming that file's root; a joint of the root file that
/// names no root goes to the output of the definition that lists its
/// occurrences.
#[test]
fn a_joint_belongs_to_the_output_of_the_definition_that_lists_its_occurrences() {
    let ir = document_with_joints();
    let names = OutputNames::from([
        (definition_id("root"), output_name("assembly.out")),
        (definition_id("part"), output_name("part.out")),
    ]);
    let planned = PlannedDocuments::default();
    let plan = planned
        .plan(EncodeInput::new(&ir, None).with_outputs(Outputs::Named(&names)))
        .unwrap();
    let documents = planned.0.into_inner().unwrap();
    assert_eq!(
        ids_of(&documents[0], "assembly_joints"),
        ["test:model:joint#root"]
    );
    let mut part_joints = ids_of(&documents[1], "assembly_joints");
    part_joints.sort();
    assert_eq!(
        part_joints,
        [
            "test:model:joint#featured",
            "test:model:joint#pin",
            "test:part@1:joint#own"
        ]
    );
    assert!(ids_of(&documents[1], "features").contains(&"test:model:feature#featured".to_owned()));
    let [message] = unheld(plan.report())[..] else {
        panic!("one record.unheld loss: {:?}", plan.report().losses)
    };
    for joint in ["local", "mixed"] {
        assert!(
            message.contains(&format!(
                r#"test:model:joint#{joint} from "assembly.out" needs test:model:occurrence#pin of "part.out""#
            )),
            "{joint} in {message}"
        );
    }
}

/// The names of `assembly.out` and `part.out` for [`two_file_document`].
fn two_names() -> OutputNames {
    OutputNames::from([
        (definition_id("root"), output_name("assembly.out")),
        (definition_id("part"), output_name("part.out")),
    ])
}

/// A record that only an output whose plan fails holds is held by no
/// output, and the report names it beside the unwritten output, which
/// names that output's definition.
#[test]
fn a_record_only_an_unwritten_output_holds_is_charged_as_unheld() {
    let ir = two_file_document_with_records();
    let names = two_names();
    let plan = HeldRecordsEncoder {
        refused: &["test:model:definition#root"],
    }
    .plan(EncodeInput::new(&ir, None).with_outputs(Outputs::Named(&names)))
    .unwrap();
    let [message] = unheld(plan.report())[..] else {
        panic!("one record.unheld loss: {:?}", plan.report().losses)
    };
    assert!(
        message.starts_with("the cadir write holds these records in no output of the encode set"),
        "{message}"
    );
    for record in ["test:model:feature#root", "test:model:tessellation#root"] {
        assert!(message.contains(record), "{record} in {message}");
    }
    assert!(!message.contains("test:model:feature#part"), "{message}");
    assert!(!message.contains("test:model:definition#root"), "{message}");
    let unwritten = plan
        .report()
        .losses
        .iter()
        .filter(|loss| code_name(&loss.code) == "export:output.unwritten")
        .map(|loss| loss.message.as_str())
        .collect::<Vec<_>>();
    assert!(
        matches!(unwritten[..], [one] if one.contains("test:model:definition#root")),
        "{unwritten:?}"
    );
}

/// A native record of no stored definition's scope that names a feature of
/// each output, and that the part's feature names by its native reference.
const LINKED: &str = "test:loose@1:records#linked";

/// The root output leaves [`LINKED`] out for the part's feature, and the
/// part output for the root's feature; the part output keeps its feature
/// without the native reference. Each output's entry states what that
/// output needs, and the partial entry follows the needs of the dropped
/// record in the same output.
#[test]
fn each_output_charges_a_record_with_its_own_needs() {
    let mut ir = two_file_document_with_records();
    ir.native
        .namespace_mut(crate::design::names::CodecId::new("test").unwrap())
        .set_arena_records(
            "records",
            vec![crate::native::NativeRecord::new(
                crate::ids::Identity::new(LINKED).unwrap(),
                [
                    (
                        "root".to_owned(),
                        serde_json::Value::String("test:model:feature#root".to_owned()),
                    ),
                    (
                        "part".to_owned(),
                        serde_json::Value::String("test:model:feature#part".to_owned()),
                    ),
                ]
                .into_iter()
                .collect(),
            )
            .unwrap()],
        )
        .unwrap();
    ir.model
        .features
        .iter_mut()
        .find(|feature| feature.id.as_str() == "test:model:feature#part")
        .unwrap()
        .native_ref = Some(LINKED.to_owned());
    let names = two_names();
    let planned = PlannedDocuments::default();
    let plan = planned
        .plan(EncodeInput::new(&ir, None).with_outputs(Outputs::Named(&names)))
        .unwrap();
    let documents = planned.0.into_inner().unwrap();
    let part = documents[1]
        .model
        .features
        .iter()
        .find(|feature| feature.id.as_str() == "test:model:feature#part")
        .expect("the part output keeps its feature");
    assert_eq!(part.native_ref, None);
    let [lost, partial] = unheld(plan.report())[..] else {
        panic!("two record.unheld losses: {:?}", plan.report().losses)
    };
    assert!(
        lost.ends_with(&format!(
            "(2): {LINKED} from \"assembly.out\" needs test:model:feature#part of \"part.out\", \
             {LINKED} from \"part.out\" needs test:model:feature#root of \"assembly.out\""
        )),
        "{lost}"
    );
    assert!(
        partial.ends_with(
            "(1): test:model:feature#part in \"part.out\" needs test:model:feature#root of \
             \"assembly.out\""
        ),
        "{partial}"
    );
}

/// A stored state of the root whose delta restates a face of the cube: the
/// part output drops the face from the delta, as it needs the cube's
/// records, and then drops the state, as it needs the root definition. The
/// part output holds no partial copy, and the root output holds the state
/// whole, so no output charges it.
#[test]
fn a_stored_state_an_output_changes_and_drops_is_not_charged_as_partial() {
    let ir = document_with_state();
    let names = two_names();
    let planned = PlannedDocuments::default();
    let plan = planned
        .plan(EncodeInput::new(&ir, None).with_outputs(Outputs::Named(&names)))
        .unwrap();
    let documents = planned.0.into_inner().unwrap();
    assert_eq!(ids_of(&documents[0], "design.evaluations"), [STATE]);
    assert!(ids_of(&documents[1], "design.evaluations").is_empty());
    for message in unheld(plan.report()) {
        assert!(!message.contains(STATE), "{message}");
    }
}

/// The stored state of [`document_with_state`] that the part output
/// changes and then drops is a dropped record of that output, and the
/// removals state no change to its delta.
#[test]
fn a_stored_state_an_output_changes_and_drops_states_no_changed_delta() {
    let ir = document_with_state();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let ctx = DecodeContext::new(&arena, &policy, DecodeDepth::Full);
    let (_, released) = crate::codec::write::encode_set::output_document(
        &ctx,
        &ir,
        Some(&definition_id("part")),
        Some(&definition_id("root")),
        &two_names(),
    )
    .unwrap();
    assert!(released.dropped.contains(STATE), "{:?}", released.dropped);
    assert!(
        released.states.keys().all(|(_, state)| state != STATE),
        "{:?}",
        released.stated()
    );
}

/// The stored state of [`document_with_state`].
const STATE: &str = "test:design:evaluation#root";

/// [`two_file_document_with_records`] with a stored state of the root whose
/// delta restates a face of the cube.
fn document_with_state() -> CadIr {
    use crate::design::evaluation::{
        Evaluation, EvaluationStatus, StateBase, StateContent, StateDelta,
    };
    use crate::design::refs::{Evaluated, Position};
    let mut ir = two_file_document_with_records();
    let face = ir.geometry.faces[0].clone();
    ir.design.evaluations.push(Evaluation {
        id: crate::design::ids::EvaluationId::mint(STATE).unwrap(),
        scope: definition_id("root"),
        configuration: None,
        of: Evaluated::State(Position::Start(
            crate::design::ids::ContainerId::mint("test:design:container#root").unwrap(),
        )),
        status: EvaluationStatus::Current,
        content: Some(StateContent {
            base: StateBase::Empty,
            delta: StateDelta {
                records: crate::document::Topology {
                    faces: vec![face],
                    ..crate::document::Topology::default()
                },
                ..StateDelta::default()
            },
        }),
        native_ref: None,
    });
    ir
}

/// A carrier that only a dropped record reached leaves the output with it,
/// for a record dropped as another output's and for one dropped because it
/// needs a record the output does not hold.
#[test]
fn an_output_holds_no_carrier_that_only_a_dropped_record_reached() {
    let mut ir = two_file_document();
    let mut free = ir.geometry.surfaces[0].clone();
    free.id = crate::ids::SurfaceId::mint("test:model:surface#free").unwrap();
    let surface = free.id.as_str().to_owned();
    ir.geometry.surfaces.push(free);
    ir.native
        .namespace_mut(crate::design::names::CodecId::new("test").unwrap())
        .set_arena_records(
            "unknowns",
            vec![crate::native::NativeRecord::new(
                crate::ids::Identity::new("test:part@1:unknowns#reach").unwrap(),
                [(
                    "links".to_owned(),
                    cadmpeg_core::json!([surface, "test:model:occurrence#part"]),
                )]
                .into_iter()
                .collect(),
            )
            .unwrap()],
        )
        .unwrap();
    let names = two_names();
    let planned = PlannedDocuments::default();
    planned
        .plan(EncodeInput::new(&ir, None).with_outputs(Outputs::Named(&names)))
        .unwrap();
    let documents = planned.0.into_inner().unwrap();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let ctx = DecodeContext::new(&arena, &policy, DecodeDepth::Full);
    for document in &documents {
        assert!(ids_of(document, "unknowns").is_empty());
        let unreached =
            crate::validate::carriers_parameterization::unreached_carriers(&ctx, document).unwrap();
        assert!(unreached.is_empty(), "{unreached:?}");
    }
}

/// A writer of a format that stores one definition per file, with a target
/// catalog. Its payload is the dialect each output resolves to.
struct PerFileCatalogEncoder;

impl EncoderBackend for PerFileCatalogEncoder {
    const FORMAT: FormatId = FormatId::new("test");
    const NAME: &'static str = "test";
    const REQUEST: DecodeDepth = DecodeDepth::Full;
    const REUSES_SOURCE: bool = false;
    const DEFINITION_PER_FILE: bool = true;
    type Target = Catalog;
    const TARGET: Catalog = Catalog::new(CATALOG_WRITE_TARGETS, Some(1));

    fn plan_resolved(
        &self,
        _: &DecodeContext<'_>,
        input: EncodeInput<'_>,
        target: ResolvedWrite<'_>,
    ) -> Result<ExportBody, CodecError> {
        Ok(ExportBody::synthesized(
            target.target_id().as_str().as_bytes().to_vec(),
            input.ir,
            ArenaCoverage::Complete,
            crate::codec::write::NativeDispositions::written(),
        ))
    }
}

/// A source of format `test` that records `dialect`, or no dialect.
fn test_source(dialect: Option<&str>) -> crate::document::SourceMeta {
    use cadmpeg_core::dialect::{DialectId, DialectLayers, DialectMatch};
    match dialect {
        Some(dialect) => crate::document::SourceMeta::classified(
            DialectLayers::of(DialectMatch::admitted(DialectId::parse(dialect).unwrap())),
            std::collections::BTreeMap::new(),
        ),
        None => json::from_value(cadmpeg_core::json!({
            "identity": {"kind": "unclassified", "format": "test"},
            "attributes": {},
        }))
        .unwrap(),
    }
}

/// [`two_file_document`] whose root file records `test:old` and whose part
/// file has `part` as its source.
fn sourced_document(part: crate::document::SourceMeta) -> CadIr {
    let mut ir = two_file_document();
    ir.source = Some(test_source(Some("test:old")));
    let definition = ir
        .model
        .product_definitions
        .iter_mut()
        .find(|definition| definition.id == definition_id("part"))
        .unwrap();
    let crate::products::Storage::File { source, .. } = &mut definition.storage else {
        panic!("the part is stored in its own file")
    };
    *source = Some(part);
    ir
}

/// The payloads and the report of an encode of `ir` under `request`.
fn per_file_targets(
    ir: &CadIr,
    request: crate::codec::write::target::TargetRequest<'_>,
) -> (Vec<String>, crate::report::export::ExportReport) {
    targets_named(ir, &two_names(), request)
}

/// The payloads and the report of an encode of `ir` under `request`, with
/// the output names `names`.
fn targets_named(
    ir: &CadIr,
    names: &OutputNames,
    request: crate::codec::write::target::TargetRequest<'_>,
) -> (Vec<String>, crate::report::export::ExportReport) {
    let plan = PerFileCatalogEncoder
        .plan(
            EncodeInput::new(ir, None)
                .with_outputs(Outputs::Named(names))
                .with_target(request),
        )
        .unwrap();
    let mut written = Vec::new();
    let report = plan
        .write_each(|_, bytes| {
            written.push(String::from_utf8_lossy(bytes).into_owned());
            Ok(())
        })
        .unwrap();
    (written, report)
}

/// The `target.*` losses of `report`, by code and message.
fn target_losses(report: &crate::report::export::ExportReport) -> Vec<(String, String)> {
    report
        .losses
        .iter()
        .filter(|loss| loss.code.local_code().starts_with("target."))
        .map(|loss| (code_name(&loss.code), loss.message.clone()))
        .collect()
}

/// An `Inherit` target resolves per output file from that file's source:
/// each output of an encode set takes its own file's dialect, the report
/// notes each output's dialect, and a `target.*` loss names the output it
/// charges (§3.4).
#[test]
fn an_inherit_target_resolves_per_output_file() {
    use crate::codec::write::target::TargetRequest;
    let (written, report) = per_file_targets(
        &sourced_document(test_source(Some("test:new"))),
        TargetRequest::Inherit,
    );
    assert_eq!(written, ["test:old", "test:new"]);
    assert_eq!(target_losses(&report), []);
    for (output, definition, dialect) in [
        ("assembly.out", "root", "test:old"),
        ("part.out", "part", "test:new"),
    ] {
        let note = format!(
            "the output \"{output}\" holds definition test:model:definition#{definition}, \
             written as {dialect};"
        );
        assert!(
            report.notes.iter().any(|stated| stated.starts_with(&note)),
            "{note} in {:?}",
            report.notes
        );
    }

    let (written, report) =
        per_file_targets(&sourced_document(test_source(None)), TargetRequest::Inherit);
    assert_eq!(written, ["test:old", "test:new"]);
    assert_eq!(
        target_losses(&report),
        [(
            "target.source-dialect-displaced".to_owned(),
            "in the output \"part.out\", the test source records no dialect; the file is \
             written as test:new"
                .to_owned()
        )]
    );
}

/// A request that names no target inherits per output file where that
/// file's source is the target's own format, and otherwise takes the
/// catalog default without a loss (§3.4).
#[test]
fn an_unnamed_target_inherits_per_output_file_where_its_source_is_this_format() {
    use crate::codec::write::target::TargetRequest;
    let (written, report) = per_file_targets(
        &sourced_document(test_source(Some("test:new"))),
        TargetRequest::Unnamed,
    );
    assert_eq!(written, ["test:old", "test:new"]);
    assert_eq!(target_losses(&report), []);
    let mut ir = two_file_document();
    ir.source = Some(test_source(Some("test:old")));
    let (written, report) = per_file_targets(&ir, TargetRequest::Unnamed);
    assert_eq!(written, ["test:old", "test:new"]);
    assert_eq!(target_losses(&report), []);
}

/// The output names in a `record.unheld` entry are quoted, so a name that
/// holds the list's separators reads as one name.
#[test]
fn an_output_name_with_list_separators_reads_as_one_name() {
    let mut ir = two_file_document_with_records();
    let part = crate::features::FeatureId::try_from("test:model:feature#part".to_owned()).unwrap();
    let mut rootdep = ir.model.features[0].clone();
    rootdep.id = "test:model:feature#rootdep".try_into().unwrap();
    rootdep.dependencies = std::iter::once(part).collect();
    ir.model.features.push(rootdep);
    let names = OutputNames::from([
        (definition_id("root"), output_name("assembly.out")),
        (definition_id("part"), output_name("a, b and c.out")),
    ]);
    let plan = PlannedDocuments::default()
        .plan(EncodeInput::new(&ir, None).with_outputs(Outputs::Named(&names)))
        .unwrap();
    let [message] = unheld(plan.report())[..] else {
        panic!("one record.unheld loss: {:?}", plan.report().losses)
    };
    assert!(
        message.ends_with(
            "(1): test:model:feature#rootdep from \"assembly.out\" needs \
             test:model:feature#part of \"a, b and c.out\""
        ),
        "{message}"
    );
}

/// A source of format `step` that records no dialect.
fn step_source() -> crate::document::SourceMeta {
    json::from_value(cadmpeg_core::json!({
        "identity": {"kind": "unclassified", "format": "step"},
        "attributes": {},
    }))
    .unwrap()
}

/// A document of the `File` definitions `files`, in arena order: each
/// keyed by its key, with its source, holding a body of its key, and
/// placing one occurrence of each definition its keys name.
fn file_document(files: &[(&str, Option<crate::document::SourceMeta>, &[&str])]) -> CadIr {
    use crate::products::{ExternalDocument, Occurrence, ProductDefinition, Storage};
    let mut ir = CadIr::empty();
    for (key, source, places) in files {
        let mut definition = ProductDefinition::new(
            definition_id(key),
            Storage::File {
                document: ExternalDocument::default(),
                source: source.clone(),
            },
        );
        let body = bare_body(key);
        definition.bodies.push(body.id.clone());
        ir.geometry.bodies.push(body);
        for placed in *places {
            let occurrence =
                crate::ids::OccurrenceId::try_from(format!("test:model:occurrence#{key}-{placed}"))
                    .unwrap();
            definition.occurrences.push(occurrence.clone());
            ir.model
                .occurrences
                .push(Occurrence::new(occurrence, definition_id(placed), None));
        }
        ir.model.product_definitions.push(definition);
    }
    ir
}

/// The key of a test id: its text after `#`.
fn key(id: &str) -> &str {
    id.rsplit_once('#').map_or(id, |(_, key)| key)
}

/// The name `<key>.out` of the output of each `File` definition of `ir`.
fn file_names(ir: &CadIr) -> OutputNames {
    ir.model
        .product_definitions
        .iter()
        .filter(|definition| matches!(definition.storage, crate::products::Storage::File { .. }))
        .map(|definition| {
            (
                definition.id.clone(),
                output_name(&format!("{}.out", key(definition.id.as_str()))),
            )
        })
        .collect()
}

/// The document of each output of `ir`, as the keys of its root
/// definitions, of its bodies, and the paths of its links, with the report.
fn planned_files(ir: &CadIr) -> (Vec<String>, crate::report::export::ExportReport) {
    let names = file_names(ir);
    let planned = PlannedDocuments::default();
    let plan = planned
        .plan(EncodeInput::new(ir, None).with_outputs(Outputs::Named(&names)))
        .unwrap();
    let documents = planned.0.into_inner().unwrap();
    let outputs = documents
        .iter()
        .map(|document| {
            let definitions = &document.model.product_definitions;
            let roots = definitions
                .iter()
                .filter(|definition| definition.is_root())
                .map(|definition| key(definition.id.as_str()))
                .collect::<Vec<_>>();
            let bodies = document
                .geometry
                .bodies
                .iter()
                .map(|body| key(body.id.as_str()))
                .collect::<Vec<_>>();
            let links = definitions
                .iter()
                .filter_map(|definition| match &definition.storage {
                    crate::products::Storage::External(document) => document
                        .path
                        .as_ref()
                        .map(cadmpeg_core::text::NonBlankString::as_str),
                    _ => None,
                })
                .collect::<Vec<_>>();
            format!(
                "{}|{}|{}",
                roots.join("+"),
                bodies.join(","),
                links.join(",")
            )
        })
        .collect();
    (outputs, plan.report().clone())
}

/// A `File` definition with no source that an occurrence places is a
/// component file whose source is unrecorded: it is an output of its own,
/// with its bodies, and the root is the definition that no occurrence
/// places, wherever it stands in the arena. A child that the root and
/// another child both place is linked from both (§8.1, §3.4).
#[test]
fn every_file_definition_other_than_the_root_is_an_output() {
    let ir = file_document(&[
        ("extra", None, &[]),
        ("root", None, &["part", "extra"]),
        ("part", Some(step_source()), &["extra"]),
    ]);
    assert_eq!(
        PlannedDocuments::default().output_definitions(&ir),
        [
            Some(definition_id("root")),
            Some(definition_id("extra")),
            Some(definition_id("part"))
        ]
    );
    let (outputs, report) = planned_files(&ir);
    assert_eq!(
        outputs,
        [
            "root|root|extra.out,part.out",
            "extra|extra|",
            "part|part|extra.out"
        ]
    );
    assert_eq!(unheld(&report), Vec::<&str>::new());
}

/// The output of a placed `File` definition with no source takes the
/// catalog default: an `Inherit` target charges
/// `target.source-dialect-displaced` naming the output, and a request that
/// names no target charges nothing (§3.4).
#[test]
fn a_file_definition_with_no_source_is_written_in_the_default_dialect() {
    use crate::codec::write::target::TargetRequest;
    let mut ir = file_document(&[
        ("extra", None, &[]),
        ("root", None, &["part", "extra"]),
        ("part", Some(test_source(Some("test:old"))), &[]),
    ]);
    ir.source = Some(test_source(Some("test:old")));
    let names = file_names(&ir);
    let (written, report) = targets_named(&ir, &names, TargetRequest::Inherit);
    assert_eq!(written, ["test:old", "test:new", "test:old"]);
    let losses = target_losses(&report);
    assert!(
        matches!(
            &losses[..],
            [(code, message)] if code == "target.source-dialect-displaced"
                && message.starts_with("in the output \"extra.out\", the document records no source;")
        ),
        "{losses:?}"
    );
    let (written, report) = targets_named(&ir, &names, TargetRequest::Unnamed);
    assert_eq!(written, ["test:old", "test:new", "test:old"]);
    assert_eq!(target_losses(&report), []);
}

/// Of several `File` definitions with no source that no occurrence places,
/// the first is the root, and each other is an output of its own.
#[test]
fn a_second_unplaced_file_definition_is_an_output_of_its_own() {
    let ir = file_document(&[("root", None, &[]), ("other", None, &[])]);
    let (outputs, report) = planned_files(&ir);
    assert_eq!(outputs, ["root|root|", "other|other|"]);
    assert_eq!(unheld(&report), Vec::<&str>::new());
}

/// Where every `File` definition is placed, the one with no source is the
/// root, and each output links the other.
#[test]
fn a_cycle_of_file_definitions_has_the_one_with_no_source_as_its_root() {
    let ir = file_document(&[
        ("part", Some(step_source()), &["root"]),
        ("root", None, &["part"]),
    ]);
    let (outputs, report) = planned_files(&ir);
    assert_eq!(outputs, ["root|root|part.out", "part|part|root.out"]);
    assert_eq!(unheld(&report), Vec::<&str>::new());
}

/// Where no `File` definition has no source, the one that no occurrence
/// places is the root: its output holds it as the root definition and
/// resolves an `Inherit` target from its own source (§3.4).
#[test]
fn a_file_definition_with_a_source_that_no_occurrence_places_is_the_root() {
    use crate::codec::write::target::TargetRequest;
    let mut ir = file_document(&[
        ("top", Some(test_source(Some("test:new"))), &["part"]),
        ("part", Some(test_source(Some("test:old"))), &[]),
    ]);
    ir.source = Some(test_source(Some("test:old")));
    let (outputs, _) = planned_files(&ir);
    assert_eq!(outputs, ["top|top|part.out", "part|part|"]);
    let (written, report) = targets_named(&ir, &file_names(&ir), TargetRequest::Inherit);
    assert_eq!(written, ["test:new", "test:old"]);
    assert_eq!(target_losses(&report), []);
}

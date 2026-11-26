// SPDX-License-Identifier: Apache-2.0
//! Encode sets (§3.4): one output per stored definition.

use crate::codec::write::{
    target::DialectFree, ArenaCoverage, ArenaDisposition, ArenaDispositions, EncodeError,
    EncodeInput, Encoder, EncoderBackend, ExportBody, OutputNames, Outputs, PlannedOutput,
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
            source: json::from_value(serde_json::json!({
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

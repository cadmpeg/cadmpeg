// SPDX-License-Identifier: Apache-2.0
//! A `SolidWorks` encode of a document that stores a part in a file of its own
//! (§3.4, Encode sets).

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::serde_erase::json;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{EncodeInput, Encoder, OutputNames, Outputs};
use cadmpeg_ir::document::{CadIr, EntityRewrite};
use cadmpeg_ir::ids::{OccurrenceId, ProductDefinitionId};
use cadmpeg_ir::products::{ExternalDocument, Occurrence, ProductDefinition, Storage};
use cadmpeg_ir::schema::rewrite::typed::{IdentityMap, RewriteIdentities};
use cadmpeg_ir::{Codec, DecodeOptions};

use crate::loss::SldprtLossCode;
use crate::SldprtCodec;

/// Moves each identity of the unit cube into the `part` namespace.
struct IntoPart<'c> {
    ctx: &'c DecodeContext<'c>,
}

impl EntityRewrite for IntoPart<'_> {
    type Error = CodecError;

    fn rewrite<T: RewriteIdentities>(&mut self, entity: T) -> Result<T, CodecError> {
        let mut map = IdentityMap::new(|id: &str| Ok(id.replacen(":cube:", ":part:", 1)));
        let entity = entity.rewrite_identities(self.ctx, &mut map)?;
        map.finish()?;
        Ok(entity)
    }
}

fn definition(key: &str) -> ProductDefinitionId {
    ProductDefinitionId::try_from(format!("test:model:definition#{key}")).unwrap()
}

/// The unit cube in its root definition, which places a second cube stored
/// in a file of its own, and an embedded definition that no occurrence
/// places.
fn assembly_and_part() -> CadIr {
    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let part_geometry = cadmpeg_ir::examples::unit_cube().unwrap().geometry;
    let first_part_body = ir.geometry.bodies.len();
    ir.geometry
        .extend_rewritten(part_geometry, &mut IntoPart { ctx: &ctx })
        .unwrap();
    let part_bodies = ir.geometry.bodies[first_part_body..]
        .iter()
        .map(|body| body.id.clone())
        .collect::<Vec<_>>();
    let mut part = ProductDefinition::new(
        definition("part"),
        Storage::File {
            document: ExternalDocument::path("part.step"),
            source: json::from_value(cadmpeg_core::json!({
                "identity": {"kind": "unclassified", "format": "step"},
                "attributes": {},
            }))
            .unwrap(),
        },
    );
    part.bodies = part_bodies;
    let occurrence = OccurrenceId::try_from("test:model:occurrence#part".to_owned()).unwrap();
    let root = ir
        .model
        .product_definitions
        .iter_mut()
        .find(|definition| definition.is_root())
        .unwrap();
    root.occurrences.push(occurrence.clone());
    ir.model.product_definitions.push(part);
    ir.model.product_definitions.push(ProductDefinition::new(
        definition("loose"),
        Storage::Embedded,
    ));
    ir.model
        .occurrences
        .push(Occurrence::new(occurrence, definition("part"), None));
    ir
}

fn decoded_faces(bytes: &[u8]) -> (usize, usize) {
    let decoded = SldprtCodec
        .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    (
        decoded.ir().geometry.bodies.len(),
        decoded.ir().geometry.faces.len(),
    )
}

#[test]
fn a_part_stored_in_its_own_file_is_written_as_an_output_of_its_own() {
    let ir = assembly_and_part();
    let root = ir
        .model
        .product_definitions
        .iter()
        .find(|definition| definition.is_root())
        .unwrap()
        .id
        .clone();
    assert_eq!(
        SldprtCodec.output_definitions(&ir),
        [Some(root.clone()), Some(definition("part"))]
    );
    let names = OutputNames::from([
        (
            root,
            cadmpeg_core::text::NonBlankString::new("assembly.sldprt").unwrap(),
        ),
        (
            definition("part"),
            cadmpeg_core::text::NonBlankString::new("part.sldprt").unwrap(),
        ),
    ]);
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None).with_outputs(Outputs::Named(&names)))
        .unwrap();
    let mut outputs = Vec::new();
    let report = plan
        .write_each(|output, bytes| {
            outputs.push((
                output.name.as_ref().map(|name| name.as_str().to_owned()),
                decoded_faces(bytes),
            ));
            Ok(())
        })
        .unwrap();
    assert_eq!(
        outputs,
        [
            (Some("assembly.sldprt".to_owned()), (1, 6)),
            (Some("part.sldprt".to_owned()), (1, 6)),
        ]
    );
    let placed = report
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "definition.placed")
        .collect::<Vec<_>>();
    assert!(placed.is_empty(), "{:?}", report.losses);
}

/// [`assembly_and_part`] as a decode of a Fusion archive states it: the part
/// is a design of the archive, and each definition holds one feature, which
/// produces the definition's body.
fn archive_of_two_designs() -> CadIr {
    use cadmpeg_ir::features::evaluation::FeatureEvaluation;
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureOperation};
    let mut ir = assembly_and_part();
    let mut produced = Vec::new();
    for stated in &mut ir.model.product_definitions {
        if let Storage::File {
            document,
            source: source @ Some(_),
        } = &mut stated.storage
        {
            *document = ExternalDocument::path("part.f3d");
            *source = json::from_value(cadmpeg_core::json!({
                "identity": {"kind": "unclassified", "format": "f3d"},
                "attributes": {},
            }))
            .unwrap();
        }
        if let Some(body) = stated.bodies.first() {
            produced.push((stated.id.clone(), body.clone()));
        }
    }
    for (ordinal, (holder, body)) in (0..).zip(produced) {
        let mut evaluation = FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::StoredGeometry {},
        ));
        evaluation.edit(|_, outputs| {
            outputs.insert(body);
        });
        ir.model.features.push(Feature {
            product_definition: Some(holder),
            id: format!("test:model:feature#{ordinal}").try_into().unwrap(),
            ordinal,
            name: None,
            suppressed: Some(false),
            dependencies: cadmpeg_ir::features::selections::DistinctMembers::default(),
            source_properties: std::collections::BTreeMap::default(),
            source_tag: None,
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),
            evaluation,
            native_ref: None,
        });
    }
    ir
}

#[test]
fn each_design_of_an_archive_is_written_with_its_own_features() {
    let ir = archive_of_two_designs();
    let root = ir
        .model
        .product_definitions
        .iter()
        .find(|definition| definition.is_root())
        .unwrap()
        .id
        .clone();
    let names = OutputNames::from([
        (
            root,
            cadmpeg_core::text::NonBlankString::new("assembly.sldprt").unwrap(),
        ),
        (
            definition("part"),
            cadmpeg_core::text::NonBlankString::new("part.sldprt").unwrap(),
        ),
    ]);
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None).with_outputs(Outputs::Named(&names)))
        .unwrap();
    let mut outputs = Vec::new();
    let report = plan
        .write_each(|output, bytes| {
            outputs.push((
                output.name.as_ref().map(|name| name.as_str().to_owned()),
                decoded_faces(bytes),
            ));
            Ok(())
        })
        .unwrap();
    assert_eq!(
        outputs,
        [
            (Some("assembly.sldprt".to_owned()), (1, 6)),
            (Some("part.sldprt".to_owned()), (1, 6)),
        ]
    );
    // Each output holds the body of each feature it writes.
    assert!(
        report
            .losses
            .iter()
            .all(|loss| loss.code != SldprtLossCode::WriterFeatureResultAbsent.kind()),
        "{:?}",
        report.losses
    );
}

#[test]
fn a_feature_whose_result_body_the_output_does_not_hold_is_left_out_and_named() {
    let mut ir = archive_of_two_designs();
    let root = ir
        .model
        .product_definitions
        .iter()
        .find(|definition| definition.is_root())
        .unwrap()
        .id
        .clone();
    ir.model
        .product_definitions
        .retain(|stated| stated.id == root);
    ir.model.product_definitions[0].occurrences.clear();
    ir.model.occurrences.clear();
    let part_bodies = ir
        .geometry
        .bodies
        .iter()
        .filter(|body| body.id.as_str().contains(":part:"))
        .map(|body| body.id.clone())
        .collect::<Vec<_>>();
    ir.geometry
        .bodies
        .retain(|body| !part_bodies.contains(&body.id));
    for feature in &mut ir.model.features {
        feature.product_definition = Some(root.clone());
    }
    let plan = SldprtCodec.plan(EncodeInput::new(&ir, None)).unwrap();
    let mut bytes = Vec::new();
    let report = plan.write_to(&mut bytes).unwrap();
    assert_eq!(decoded_faces(&bytes).0, 1);
    let absent = report
        .losses
        .iter()
        .find(|loss| loss.code == SldprtLossCode::WriterFeatureResultAbsent.kind())
        .unwrap_or_else(|| panic!("{:?}", report.losses));
    assert!(
        absent.message.contains("test:model:feature#1"),
        "{}",
        absent.message
    );
}

#[test]
fn an_encode_that_names_no_output_of_the_part_is_refused() {
    let ir = assembly_and_part();
    let error = SldprtCodec.plan(EncodeInput::new(&ir, None)).unwrap_err();
    let cadmpeg_ir::codec::write::EncodeError::OutputUnnamed {
        definition: unnamed,
    } = error
    else {
        panic!("the part's output has no name, got {error}")
    };
    assert_eq!(unnamed, definition("part"));
}

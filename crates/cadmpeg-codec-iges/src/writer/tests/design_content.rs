// SPDX-License-Identifier: Apache-2.0
//! Design content IGES has no place for is left out and reported; the shape
//! beside it is written (§3.4, Content the target cannot hold).

use std::collections::BTreeMap;
use std::io::Cursor;

use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::features::parameters::DesignParameter;
use cadmpeg_ir::features::selections::DistinctMembers;
use cadmpeg_ir::features::ParameterId;
use cadmpeg_ir::CadIr;

use crate::test_support::test_solids_and_structure::explicit_tetrahedron_solid_file;
use crate::{IgesCodec, IgesVersion};

/// A tetrahedron solid as a document of another codec states it: with no
/// IGES native records and no entity labels or levels.
fn tetrahedron() -> CadIr {
    let mut ir = IgesCodec
        .decode(
            &mut Cursor::new(explicit_tetrahedron_solid_file()),
            &DecodeOptions::default(),
        )
        .expect("decode")
        .ir()
        .clone();
    ir.native = cadmpeg_ir::native::Native::default();
    ir.model.attributes.clear();
    ir.model.presentation_layers.clear();
    ir
}

/// Adds one feature, one parameter and one attribute on the first body: the
/// design content a part from a history-based system holds beside its shape.
fn with_design_content(ir: &mut CadIr) {
    ir.model.features.push(
        serde_json::from_value(serde_json::json!({
            "id": "cadir:model:feature#boss", "ordinal": 0, "name": "Boss", "suppressed": false,
            "definition": {"kind": "operation", "value": {"kind": "stored_geometry", "value": {}}},
            "outputs": [], "dependencies": [], "source_properties": {},
            "source_content": []
        }))
        .expect("feature"),
    );
    ir.model.parameters.push(DesignParameter {
        product_definition: None,
        id: ParameterId::mint("cadir:model:parameter#height").expect("identity"),
        owner: None,
        ordinal: 0,
        name: "Height".into(),
        expression: "12mm".into(),
        display: None,
        value: None,
        dependencies: DistinctMembers::default(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });
    ir.model.attributes.push(SourceAttribute {
        id: "cadir:model:attribute#finish".try_into().expect("identity"),
        target: AttributeTarget::Body(ir.geometry.bodies[0].id.clone()).into(),
        name: "Finish".into(),
        values: vec![Some(AttributeValue::String("anodized".into()))],
        native_ref: None,
    });
}

#[test]
fn a_solid_with_design_content_is_written_and_the_content_reported() {
    let mut ir = tetrahedron();
    with_design_content(&mut ir);

    let plan = IgesCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(IgesVersion::V5_3.descriptor().id.as_str()),
        )
        .expect("the solid is writable");
    let mut bytes = Vec::new();
    let report = plan.write_to(&mut bytes).expect("write");
    let omitted = report
        .losses
        .iter()
        .filter(|loss| loss.code.namespace() == "export")
        .map(|loss| loss.message.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        omitted,
        [
            "the iges write does not represent model arena `features`: 1 record(s) omitted: cadir:model:feature#boss",
            "the iges write does not represent model arena `parameters`: 1 record(s) omitted: cadir:model:parameter#height",
            "the iges write does not represent model arena `attributes`: 1 record(s) omitted: cadir:model:attribute#finish",
        ]
    );
    let decoded = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("the written file decodes");
    assert_eq!(decoded.ir().geometry.bodies.len(), 1);
    assert_eq!(decoded.ir().geometry.faces.len(), 4);
}

#[test]
fn a_native_record_synthesis_does_not_encode_is_named_not_refused() {
    let source = include_bytes!("../../../tests/golden/fixtures/colored_explicit_vertex_loop.igs");
    let decoded = IgesCodec
        .decode(&mut Cursor::new(source.to_vec()), &DecodeOptions::default())
        .expect("decode");
    let colors = decoded
        .ir()
        .native
        .namespace("iges")
        .and_then(|namespace| namespace.arenas().get("colors"))
        .map_or(0, Vec::len);
    assert_ne!(colors, 0, "the fixture holds a native color record");

    let plan = IgesCodec
        .plan(
            EncodeInput::new(decoded.ir(), None),
            TargetRequest::Explicit(IgesVersion::V5_3.descriptor().id.as_str()),
        )
        .expect("the loop is written without its native color records");
    assert!(
        plan.report()
            .losses
            .iter()
            .any(|loss| loss.message.starts_with(&format!(
                "the IGES write omits {colors} native record(s) of `iges` arena `colors` which \
                 the semantic writer does not encode:"
            ))),
        "{:?}",
        plan.report().losses
    );
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("write");
    let written = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("the written file decodes");
    assert_eq!(
        written.ir().geometry.faces.len(),
        decoded.ir().geometry.faces.len()
    );
}

#[test]
fn a_given_up_part_stub_is_not_charged() {
    let source = include_bytes!("../../../tests/golden/fixtures/colored_explicit_vertex_loop.igs");
    let mut ir = IgesCodec
        .decode(&mut Cursor::new(source.to_vec()), &DecodeOptions::default())
        .expect("decode")
        .into_parts()
        .0;
    ir.native
        .namespace_mut(cadmpeg_ir::codec_id!("iges"))
        .arenas_mut()
        .insert(
            cadmpeg_core::nonblank_literal!("unknowns"),
            vec![cadmpeg_ir::NativeRecord::new(
                cadmpeg_ir::ids::Identity::new("iges:test:unknown#0").expect("valid identity"),
                serde_json::Map::default(),
            )
            .expect("valid native identity")],
        );
    ir.finalize();

    let plan = IgesCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(IgesVersion::V5_3.descriptor().id.as_str()),
        )
        .expect("the stub does not refuse the write");
    assert!(
        !plan
            .report()
            .losses
            .iter()
            .any(|loss| loss.message.contains("iges:test:unknown#0")),
        "{:?}",
        plan.report().losses
    );
}

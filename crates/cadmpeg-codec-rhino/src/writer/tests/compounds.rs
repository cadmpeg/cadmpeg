// SPDX-License-Identifier: Apache-2.0
//! Compound reductions retain their admitted child geometry.

use crate::{RhinoArchiveVersion, RhinoCodec};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::{EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::{
    CompoundComponent, CompoundCurveConstruction, ProceduralCurveDefinition,
};
use cadmpeg_ir::report::Severity;
use std::io::Cursor;

fn compound_document() -> cadmpeg_ir::CadIr {
    RhinoCodec
        .decode(
            &mut Cursor::new(include_bytes!(
                "../../../tests/golden/fixtures/polycurve.3dm"
            )),
            &DecodeOptions::default(),
        )
        .expect("compound fixture")
        .ir()
        .clone()
}

#[test]
fn unsupported_compound_convention_keeps_writable_components() {
    let mut ir = compound_document();
    let children = crate::writer::compounds::children(&ir);
    let parent = ir
        .model
        .curves
        .iter()
        .find(|curve| {
            curve.geometry.procedural_construction().is_some() && !children.contains(&curve.id)
        })
        .expect("root compound");
    let construction = parent
        .geometry
        .procedural_construction()
        .expect("construction")
        .clone();
    let parent_id = parent.id.to_string();
    ir.model
        .procedural_curves
        .iter_mut()
        .find(|record| record.id == construction)
        .expect("held construction")
        .edit_definition(|definition| {
            let ProceduralCurveDefinition::Compound(compound) = definition else {
                panic!("compound")
            };
            let components = compound
                .components()
                .iter()
                .map(|component| CompoundComponent {
                    parameter: component.parameter.get(),
                    component: component.component.clone(),
                })
                .collect();
            *compound = CompoundCurveConstruction::try_new(Vec::new(), components, None)
                .expect("valid neutral convention without native interval spelling");
        });
    let mut bytes = Vec::new();
    let report = RhinoCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .expect("degraded plan")
        .write_to(&mut bytes)
        .expect("degraded archive");
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.severity == Severity::Error && loss.message.contains(&parent_id)));
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("children remain valid native objects");
    assert_eq!(
        decoded.ir().model.curves.len(),
        ir.model.curves.len() - 1,
        "only the unrepresentable enclosing compound is omitted"
    );
}

#[test]
fn compound_cycle_is_not_hidden_when_it_has_no_standalone_root() {
    let mut ir = compound_document();
    let children = crate::writer::compounds::children(&ir);
    let parent = ir
        .model
        .curves
        .iter()
        .find(|curve| {
            curve.geometry.procedural_construction().is_some() && !children.contains(&curve.id)
        })
        .expect("root compound");
    let construction = parent
        .geometry
        .procedural_construction()
        .expect("construction")
        .clone();
    let parent_id = parent.id.clone();
    ir.model
        .procedural_curves
        .iter_mut()
        .find(|record| record.id == construction)
        .expect("held construction")
        .replace_definition(ProceduralCurveDefinition::Compound(
            CompoundCurveConstruction::try_new(
                vec![0.0, 1.0],
                vec![CompoundComponent {
                    parameter: 0.0,
                    component: parent_id,
                }],
                None,
            )
            .expect("typed cyclic graph"),
        ));
    let error = RhinoCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .expect_err("cyclic document");
    assert!(matches!(error, CodecError::Malformed(message) if message.contains("cyclic compound")));
}

// SPDX-License-Identifier: Apache-2.0
//! Temporary probe: selector re-decode.

use std::io::Cursor;

use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::FcstdCodec;

#[test]
fn probe_mesh_drop() {
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(crate::test_support::test_archive::CORE_DESIGN_PRODUCT),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut ir = decoded.ir().clone();
    let owner = ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Box"))
        .unwrap()
        .id
        .clone();
    let parameter = ir
        .model
        .parameters
        .iter_mut()
        .find(|parameter| {
            parameter.owner.as_ref() == Some(&owner) && parameter.name == "Length"
        })
        .unwrap();
    let id = parameter.id.clone();
    parameter.name = "9 depth".into();
    let plan = FcstdCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Explicit("fcstd:schema-4"))
        .unwrap();
    let report = plan.report().clone();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    eprintln!("looking for {id}");
    for parameter in &output.ir().model.parameters {
        if parameter.owner.as_ref() == Some(
            &output.ir().model.features.iter().find(|f| f.name.as_deref() == Some("Box")).unwrap().id,
        ) {
            eprintln!("BOX PARAM {} name={} native={:?}", parameter.id, parameter.name, parameter.native_ref);
        }
    }
    for loss in report.losses.iter().filter(|l| l.code.to_string().contains("parameter")) {
        eprintln!("LOSS {}: {}", loss.code, loss.message);
    }
}

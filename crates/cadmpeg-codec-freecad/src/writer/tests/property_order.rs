// SPDX-License-Identifier: Apache-2.0
//! Declaration order of regenerated native properties.

use crate::FcstdCodec;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
fn synthesized_feature_keeps_surviving_property_order() {
    let input = FcstdCodec
        .decode(
            &mut Cursor::new(crate::test_support::test_archive::CORE_DESIGN_PRODUCT),
            &DecodeOptions::default(),
        )
        .unwrap();
    let plan = FcstdCodec
        .plan(EncodeInput::new(input.ir(), None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let properties = |ir: &cadmpeg_ir::CadIr| {
        ir.native
            .namespace("fcstd")
            .unwrap()
            .arena_as::<crate::native::PropertyRecord>("properties")
            .unwrap()
            .into_iter()
            .filter(|property| property.owner == "fcstd:native:object#Box")
            .collect::<Vec<_>>()
    };
    let source = properties(input.ir());
    let emitted = properties(output.ir());
    let mut common_source: Vec<_> = source
        .iter()
        .filter(|property| emitted.iter().any(|other| other.id == property.id))
        .collect();
    let mut common_output: Vec<_> = emitted
        .iter()
        .filter(|property| source.iter().any(|other| other.id == property.id))
        .collect();
    let key = |property: &&crate::native::PropertyRecord| {
        (
            !matches!(property.body, Some(crate::native::PropertyBody::Transient)),
            property.order,
        )
    };
    common_source.sort_by_key(key);
    common_output.sort_by_key(key);
    assert!(common_source.len() > 3);
    assert_eq!(
        common_source
            .iter()
            .map(|property| &property.id)
            .collect::<Vec<_>>(),
        common_output
            .iter()
            .map(|property| &property.id)
            .collect::<Vec<_>>()
    );
}

#[test]
fn parameter_ordinals_override_original_property_positions() {
    let input = FcstdCodec
        .decode(
            &mut Cursor::new(crate::test_support::test_archive::CORE_DESIGN_PRODUCT),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut ir = input.ir().clone();
    let owner = ir
        .model
        .features
        .iter()
        .find(|feature| feature.native_ref.as_deref() == Some("fcstd:native:object#Box"))
        .unwrap()
        .id
        .clone();
    let mut edited = 0;
    for parameter in ir
        .model
        .parameters
        .iter_mut()
        .filter(|parameter| parameter.owner.as_ref() == Some(&owner))
    {
        parameter.ordinal = match parameter.name.as_str() {
            "Height" => 2,
            "Length" => 1,
            "Width" => 0,
            _ => continue,
        };
        edited += 1;
    }
    assert_eq!(edited, 3);
    let plan = FcstdCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let mut properties: Vec<_> = output
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap()
        .into_iter()
        .filter(|property| {
            property.owner == "fcstd:native:object#Box"
                && matches!(property.name.as_str(), "Height" | "Length" | "Width")
        })
        .collect();
    properties.sort_by_key(|property| property.order);
    assert_eq!(
        properties
            .iter()
            .map(|property| property.name.as_str())
            .collect::<Vec<_>>(),
        ["Width", "Length", "Height"]
    );
}

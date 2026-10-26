// SPDX-License-Identifier: Apache-2.0
use super::*;
use cadmpeg_ir::features::PlanarProfileRef;

#[test]
fn native_revolve_losses_identify_the_unresolved_form() {
    for (reason, code, detail) in [
        (
            NativeReason::Slot(16),
            "feature.revolve-operands-native",
            "slot 16",
        ),
        (
            NativeReason::Extent(7),
            "feature.revolve-extent-native",
            "extent value 7",
        ),
        (
            NativeReason::Direction(1),
            "feature.revolve-direction-native",
            "direction value 1",
        ),
    ] {
        let note = reason.note();
        assert_eq!(note.code.local_code(), code);
        assert!(note.message.contains(detail));
    }
}

fn dimension(bytes: &[u8]) -> Result<DimensionPayload, CodecError> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, source) = DecodeContext::from_root_bytes(
        bytes,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("dimension context");
    parse_dimension(&ctx, source)
}

#[test]
fn dimension_records_require_the_complete_frame_and_preserve_the_parameter() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&37u16.to_le_bytes());
    bytes.extend_from_slice(&0x3000_0002u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.push(7);
    bytes.extend_from_slice(&19u32.to_le_bytes());
    bytes.extend_from_slice(&0x8000_0043u32.to_le_bytes());
    let parsed = dimension(&bytes).expect("dimension");
    assert_eq!(parsed.header_id, 37);
    assert_eq!(parsed.annotation.into_inner(), [7, 19, 0, 0, 0]);
    assert_eq!(parsed.parameter.index(), 67);
    assert_eq!(parsed.item_count, 0);
    assert!(parsed.items.into_inner().is_empty());
    for end in 0..bytes.len() {
        assert!(dimension(&bytes[..end]).is_err());
    }
    bytes.push(0);
    assert!(dimension(&bytes).is_err());
}

#[test]
fn neutral_revolve_axes_extents_and_output_modes_must_match_the_native_operands() {
    let expected = Operands {
        axis: RevolutionAxis {
            origin: cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0))
                .expect("origin"),
            direction: cadmpeg_ir::features::FeatureDirection3::from(
                cadmpeg_ir::units::UnitVector3::normalized(Vector3::new(0.0, 1.0, 0.0))
                    .expect("axis"),
            ),
            reference: None,
        },
        extent: RevolveExtent::OneSided {
            termination: AngularTermination::Angle {
                angle: cadmpeg_ir::scalar::PositiveAngle::new(std::f64::consts::TAU)
                    .expect("full turn"),
            },
        },
        op: BooleanOp::Cut,
        unresolved_control: false,
    };
    let construction = RevolveConstruction::Resolved {
        profile: PlanarProfileRef::from(
            cadmpeg_ir::sketches::SketchId::mint("inventor:design:sketch#synthetic")
                .expect("sketch identity"),
        ),
        axis: expected.axis.clone(),
        extent: expected.extent.clone(),
        solid: Some(true),
        face_maker: None,
        fuse_order: None,
        allow_multi_profile_faces: None,
    };
    assert!(matches(&expected, &construction, BooleanOp::Cut));
    assert!(!matches(&expected, &construction, BooleanOp::Join));
    for field in ["axis", "extent", "solid"] {
        let mut changed = construction.clone();
        let RevolveConstruction::Resolved {
            axis,
            extent,
            solid,
            ..
        } = &mut changed
        else {
            panic!("resolved construction");
        };
        match field {
            "axis" => {
                axis.origin = cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 0.0, 0.0))
                    .expect("shifted origin");
            }
            "extent" => {
                *extent = RevolveExtent::OneSided {
                    termination: AngularTermination::Angle {
                        angle: cadmpeg_ir::scalar::PositiveAngle::new(1.0).expect("partial turn"),
                    },
                }
            }
            "solid" => *solid = Some(false),
            _ => unreachable!(),
        }
        assert!(
            !matches(&expected, &changed, BooleanOp::Cut),
            "changed {field}"
        );
    }
}

#[test]
fn dimension_item_counts_are_bounded_before_allocation() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&37u16.to_le_bytes());
    bytes.extend_from_slice(&0x3000_0002u32.to_le_bytes());
    let count_position = bytes.len();
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0x8000_000bu32.to_le_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0x8000_0043u32.to_le_bytes());
    let parsed = dimension(&bytes).expect("dimension annotation list");
    assert_eq!(parsed.item_count, 1);
    assert_eq!(parsed.items.into_inner(), 0x8000_000bu32.to_le_bytes());
    assert_eq!(parsed.parameter.index(), 67);
    bytes[count_position..count_position + std::mem::size_of::<u32>()]
        .copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(dimension(&bytes).is_err());
}

#[test]
fn dimension_annotation_data_precedes_the_parameter_tail() {
    for data_len in [24, 128] {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&37u16.to_le_bytes());
        bytes.extend_from_slice(&0x3000_0002u32.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.resize(bytes.len() + data_len, 0x5a);
        bytes.extend_from_slice(&[7, 19, 0, 0, 0]);
        bytes.extend_from_slice(&0x8000_0043u32.to_le_bytes());
        let parsed = dimension(&bytes).expect("annotation frame");
        assert_eq!(parsed.item_count, 1);
        let items = parsed.items.into_inner();
        assert_eq!(items.len(), data_len);
        assert!(items.iter().all(|byte| *byte == 0x5a));
        assert_eq!(parsed.annotation.into_inner(), [7, 19, 0, 0, 0]);
        assert_eq!(parsed.parameter.index(), 67);
    }
}

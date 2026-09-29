// SPDX-License-Identifier: Apache-2.0

use crate::native::features::payload_content::{FeaturePayloadBlock, FeaturePayloadContent};
use crate::native::features::{
    feature_datum_csys_payload_fixed_pairs, feature_datum_csys_payload_scalar_pairs,
    feature_datum_csys_payload_scalars, feature_datum_plane_payload_scalar_pairs,
    feature_sketch_payload_coordinate_pairs, feature_sketch_payload_fixed_pairs,
    feature_sketch_payload_mixed_pairs, feature_sketch_payload_named_records,
    feature_sketch_payload_names, feature_sketch_payload_scalar_lanes,
    feature_surface_construction_scalar_pairs, feature_surface_construction_strings,
    offset_data_block_bytes, FeatureConstructionOwner, FeatureConstructionPayload,
    FeatureDatumCsysPayload, FeatureDatumPlanePayload, FeaturePayloadName,
    FeatureSketchConstructionInputs, FeatureSurfaceConstructionPayload,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[derive(Clone, Copy)]
enum FrameRoute {
    CsysPair,
    CsysFixed,
    CsysScalar,
    PlanePair,
    SketchPair,
    SketchFixed,
    SketchMixed,
    SketchLane,
    SketchName,
    SketchRecord,
    SurfacePair,
    SurfaceString,
}

enum FramePayload {
    Csys(FeatureDatumCsysPayload),
    Plane(FeatureDatumPlanePayload),
    Sketch(FeatureConstructionPayload),
    SketchInputs {
        inputs: FeatureSketchConstructionInputs,
        payload: FeatureConstructionPayload,
        names: Vec<FeaturePayloadName>,
    },
    Surface(Box<FeatureSurfaceConstructionPayload>),
}

fn frame_bytes(route: FrameRoute) -> Vec<u8> {
    use crate::test_support::test_bytes::shifted_f64_bytes;
    let mut bytes = match route {
        FrameRoute::CsysPair | FrameRoute::SketchPair | FrameRoute::SurfacePair => {
            vec![8, 2, 3, 1, 3, 1, 0xc0, 0x45, 4, 0, 0x80, 0x86, 2, 0, 3]
        }
        FrameRoute::CsysFixed => vec![0x0b, 2, 3, 1, 3, 1, 0xc0, 0x45, 4, 0, 0x80, 0x86, 2, 0, 3],
        FrameRoute::CsysScalar => {
            return vec![
                0x50, 0x59, 0x66, 0x64, 0, 0x30, 0x43, 0x0c, 0xcc, 0xcc, 0xcc, 0xcd, 0x72,
            ]
        }
        FrameRoute::SurfaceString => return b"\x66\x1b\x03\x05Steel\0".to_vec(),
        FrameRoute::SketchName | FrameRoute::SketchRecord => return b"\x03\x05ABC\0".to_vec(),
        FrameRoute::PlanePair => vec![
            0x6d, 0, 0xf0, 8, 2, 3, 1, 3, 1, 0xc0, 0x45, 4, 0, 0x80, 0x86, 2, 0, 3,
        ],
        FrameRoute::SketchFixed | FrameRoute::SketchMixed => {
            vec![0x04, 0xe0, 0x48, 0x0e, 2, 3, 0x80, 0x84]
        }
        FrameRoute::SketchLane => {
            let mut lane = vec![
                0x25, 0x25, 0x41, 0, 4, 1, 7, 1, 0xc0, 0x45, 0x10, 0, 0x80, 0x86, 2, 0, 1, 0,
            ];
            lane.extend_from_slice(&shifted_f64_bytes(1.5));
            let mut binary32 = 3.25_f32.to_be_bytes();
            binary32[0] += 0x10;
            lane.extend_from_slice(&binary32);
            lane.push(0);
            return lane;
        }
    };
    bytes.extend_from_slice(&shifted_f64_bytes(2.0));
    bytes.push(0);
    if matches!(route, FrameRoute::SketchMixed) {
        bytes.extend_from_slice(&[0x50, 0x50, 0, 0]);
    } else {
        bytes.extend_from_slice(&shifted_f64_bytes(3.0));
    }
    bytes
}

fn frame_fixture(route: FrameRoute) -> (crate::container::Container<'static>, FramePayload) {
    let bytes = frame_bytes(route);
    let store = (0..600).map(|_| bytes.as_slice()).collect::<Vec<_>>();
    let part = crate::test_support::test_om::composed_feature_history_payload(
        &[(
            &[0xff; 4],
            "SKIN",
            super::reference_admission::surface_payload_bytes(),
        )],
        &store,
    );
    let file =
        crate::test_support::test_prt::prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", part)]);
    let container = crate::test_support::with_decode_context(move |ctx| {
        crate::container::scan_bytes(ctx, file)
    })
    .expect("synthetic frame container");
    let payload = crate::test_support::with_decode_context(|ctx| {
        let blocks = offset_data_block_bytes(ctx, &container)?;
        let id = blocks
            .keys()
            .find(|key| key.ends_with(":block#1"))
            .cloned()
            .expect("synthetic block identity");
        Ok::<FramePayload, CodecError>(match route {
            FrameRoute::CsysPair | FrameRoute::CsysFixed | FrameRoute::CsysScalar => {
                let content: FeaturePayloadContent<[FeaturePayloadBlock; 2]> =
                    FeaturePayloadContent::from_source(ctx, [id.clone(), id], &blocks)?
                        .expect("two-block payload");
                FramePayload::Csys(FeatureDatumCsysPayload {
                    id: "csys-payload".into(),
                    operation_label: "operation".into(),
                    construction: "construction".into(),
                    content,
                })
            }
            FrameRoute::PlanePair => {
                let content: FeaturePayloadContent<Vec<FeaturePayloadBlock>> =
                    FeaturePayloadContent::from_source(ctx, [id], &blocks)?.expect("plane payload");
                FramePayload::Plane(FeatureDatumPlanePayload {
                    id: "plane-payload".into(),
                    operation_label: "operation".into(),
                    datum_plane_header: "header".into(),
                    content,
                    index_lane: None,
                })
            }
            FrameRoute::SketchPair
            | FrameRoute::SketchFixed
            | FrameRoute::SketchMixed
            | FrameRoute::SketchLane => {
                let content: FeaturePayloadContent<Vec<FeaturePayloadBlock>> =
                    FeaturePayloadContent::from_source(ctx, [id], &blocks)?
                        .expect("sketch payload");
                FramePayload::Sketch(FeatureConstructionPayload {
                    id: "sketch-payload".into(),
                    operation_label: "operation".into(),
                    owner: FeatureConstructionOwner::Sketch {
                        construction_inputs: "inputs".into(),
                    },
                    content,
                })
            }
            FrameRoute::SketchName | FrameRoute::SketchRecord => {
                let content: FeaturePayloadContent<Vec<FeaturePayloadBlock>> =
                    FeaturePayloadContent::from_source(ctx, [id.clone()], &blocks)?
                        .expect("sketch named record payload");
                let inputs = serde_json::json!({
                    "id": "nx:feature-history:sketch-construction-inputs#0-0000000000",
                    "operation_label": "operation", "sketch_record": "record",
                    "member_references": [], "member_data_blocks": [],
                    "terminal_reference": "terminal", "terminal_data_block": id,
                });
                let inputs: FeatureSketchConstructionInputs =
                    serde_json::from_value(inputs).expect("sketch name inputs");
                let names =
                    feature_sketch_payload_names(ctx, &container, std::slice::from_ref(&inputs))?;
                FramePayload::SketchInputs {
                    inputs,
                    payload: FeatureConstructionPayload {
                        id: "nx:feature-history:sketch-construction-payload#0-0000000000".into(),
                        operation_label: "operation".into(),
                        owner: FeatureConstructionOwner::Sketch {
                            construction_inputs: "inputs".into(),
                        },
                        content,
                    },
                    names,
                }
            }
            FrameRoute::SurfacePair | FrameRoute::SurfaceString => {
                let content: FeaturePayloadContent<[FeaturePayloadBlock; 14]> =
                    FeaturePayloadContent::from_source(ctx, vec![id; 14], &blocks)?
                        .expect("surface payload");
                FramePayload::Surface(Box::new(FeatureSurfaceConstructionPayload {
                    id: "surface-payload".into(),
                    operation_label: "operation".into(),
                    construction_references: std::array::from_fn(|_| "reference".into()),
                    content,
                }))
            }
        })
    })
    .expect("synthetic frame payload");
    (container, payload)
}

fn run_frame_route(
    route: FrameRoute,
    ctx: &DecodeContext<'_>,
    container: &crate::container::Container<'_>,
    payload: &FramePayload,
) -> Result<usize, CodecError> {
    match (route, payload) {
        (FrameRoute::CsysPair, FramePayload::Csys(payload)) => {
            feature_datum_csys_payload_scalar_pairs(ctx, container, std::slice::from_ref(payload))
                .map(|rows| rows.len())
        }
        (FrameRoute::CsysFixed, FramePayload::Csys(payload)) => {
            feature_datum_csys_payload_fixed_pairs(ctx, container, std::slice::from_ref(payload))
                .map(|rows| rows.len())
        }
        (FrameRoute::CsysScalar, FramePayload::Csys(payload)) => {
            feature_datum_csys_payload_scalars(ctx, container, std::slice::from_ref(payload))
                .map(|rows| rows.len())
        }
        (FrameRoute::PlanePair, FramePayload::Plane(payload)) => {
            feature_datum_plane_payload_scalar_pairs(ctx, container, std::slice::from_ref(payload))
                .map(|rows| rows.len())
        }
        (FrameRoute::SketchPair, FramePayload::Sketch(payload)) => {
            feature_sketch_payload_coordinate_pairs(ctx, container, std::slice::from_ref(payload))
                .map(|rows| rows.len())
        }
        (FrameRoute::SketchFixed, FramePayload::Sketch(payload)) => {
            feature_sketch_payload_fixed_pairs(ctx, container, std::slice::from_ref(payload))
                .map(|rows| rows.len())
        }
        (FrameRoute::SketchMixed, FramePayload::Sketch(payload)) => {
            feature_sketch_payload_mixed_pairs(ctx, container, std::slice::from_ref(payload))
                .map(|rows| rows.len())
        }
        (FrameRoute::SketchLane, FramePayload::Sketch(payload)) => {
            feature_sketch_payload_scalar_lanes(ctx, container, std::slice::from_ref(payload))
                .map(|rows| rows.len())
        }
        (FrameRoute::SketchName, FramePayload::SketchInputs { inputs, .. }) => {
            feature_sketch_payload_names(ctx, container, std::slice::from_ref(inputs))
                .map(|rows| rows.len())
        }
        (FrameRoute::SketchRecord, FramePayload::SketchInputs { payload, names, .. }) => {
            feature_sketch_payload_named_records(
                ctx,
                std::slice::from_ref(payload),
                names,
                &[],
                &[],
                &[],
            )
            .map(|rows| rows.len())
        }
        (FrameRoute::SurfacePair, FramePayload::Surface(payload)) => {
            feature_surface_construction_scalar_pairs(
                ctx,
                container,
                std::slice::from_ref(payload.as_ref()),
            )
            .map(|rows| rows.len())
        }
        (FrameRoute::SurfaceString, FramePayload::Surface(payload)) => {
            feature_surface_construction_strings(
                ctx,
                container,
                std::slice::from_ref(payload.as_ref()),
            )
            .map(|rows| rows.len())
        }
        _ => panic!("frame route fixture mismatch"),
    }
}

fn assert_frame_limit(route: FrameRoute, dimension: ResourceDimension) {
    let (container, payload) = frame_fixture(route);
    let count = crate::test_support::with_decode_context(|ctx| {
        run_frame_route(route, ctx, &container, &payload)
    })
    .expect("frame route baseline");
    assert!(count > 0, "frame fixture must reach the output record");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    match dimension {
        ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
        ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = 0,
        ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
        ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
        _ => panic!("unsupported frame test dimension"),
    }
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty test root");
    let error =
        run_frame_route(route, &ctx, &container, &payload).expect_err("frame limit refusal");
    assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == dimension));
}

macro_rules! frame_limit_tests {
    ($route:expr, $collection:ident, $retained:ident, $scoped:ident, $work:ident) => {
        #[test]
        fn $collection() {
            assert_frame_limit($route, ResourceDimension::CollectionItems);
        }
        #[test]
        fn $retained() {
            assert_frame_limit($route, ResourceDimension::RetainedBytes);
        }
        #[test]
        fn $scoped() {
            assert_frame_limit($route, ResourceDimension::MaterializedBytes);
        }
        #[test]
        fn $work() {
            assert_frame_limit($route, ResourceDimension::WorkUnits);
        }
    };
}

frame_limit_tests!(
    FrameRoute::CsysPair,
    csys_pair_frame_refuses_collection_limit,
    csys_pair_frame_refuses_retained_limit,
    csys_pair_frame_refuses_scoped_limit,
    csys_pair_frame_refuses_work_limit
);
frame_limit_tests!(
    FrameRoute::CsysFixed,
    csys_fixed_frame_refuses_collection_limit,
    csys_fixed_frame_refuses_retained_limit,
    csys_fixed_frame_refuses_scoped_limit,
    csys_fixed_frame_refuses_work_limit
);
frame_limit_tests!(
    FrameRoute::CsysScalar,
    csys_scalar_frame_refuses_collection_limit,
    csys_scalar_frame_refuses_retained_limit,
    csys_scalar_frame_refuses_scoped_limit,
    csys_scalar_frame_refuses_work_limit
);
frame_limit_tests!(
    FrameRoute::PlanePair,
    plane_pair_frame_refuses_collection_limit,
    plane_pair_frame_refuses_retained_limit,
    plane_pair_frame_refuses_scoped_limit,
    plane_pair_frame_refuses_work_limit
);
frame_limit_tests!(
    FrameRoute::SketchPair,
    sketch_pair_frame_refuses_collection_limit,
    sketch_pair_frame_refuses_retained_limit,
    sketch_pair_frame_refuses_scoped_limit,
    sketch_pair_frame_refuses_work_limit
);
frame_limit_tests!(
    FrameRoute::SketchFixed,
    sketch_fixed_frame_refuses_collection_limit,
    sketch_fixed_frame_refuses_retained_limit,
    sketch_fixed_frame_refuses_scoped_limit,
    sketch_fixed_frame_refuses_work_limit
);
frame_limit_tests!(
    FrameRoute::SketchMixed,
    sketch_mixed_frame_refuses_collection_limit,
    sketch_mixed_frame_refuses_retained_limit,
    sketch_mixed_frame_refuses_scoped_limit,
    sketch_mixed_frame_refuses_work_limit
);
frame_limit_tests!(
    FrameRoute::SketchLane,
    sketch_lane_frame_refuses_collection_limit,
    sketch_lane_frame_refuses_retained_limit,
    sketch_lane_frame_refuses_scoped_limit,
    sketch_lane_frame_refuses_work_limit
);
frame_limit_tests!(
    FrameRoute::SketchName,
    sketch_name_frame_refuses_collection_limit,
    sketch_name_frame_refuses_retained_limit,
    sketch_name_frame_refuses_scoped_limit,
    sketch_name_frame_refuses_work_limit
);
frame_limit_tests!(
    FrameRoute::SketchRecord,
    sketch_record_frame_refuses_collection_limit,
    sketch_record_frame_refuses_retained_limit,
    sketch_record_frame_refuses_scoped_limit,
    sketch_record_frame_refuses_work_limit
);
frame_limit_tests!(
    FrameRoute::SurfacePair,
    surface_pair_frame_refuses_collection_limit,
    surface_pair_frame_refuses_retained_limit,
    surface_pair_frame_refuses_scoped_limit,
    surface_pair_frame_refuses_work_limit
);
frame_limit_tests!(
    FrameRoute::SurfaceString,
    surface_string_frame_refuses_collection_limit,
    surface_string_frame_refuses_retained_limit,
    surface_string_frame_refuses_scoped_limit,
    surface_string_frame_refuses_work_limit
);

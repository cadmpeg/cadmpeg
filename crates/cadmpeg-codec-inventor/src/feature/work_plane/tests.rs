// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, NativeFeatureKind};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::FiniteReal;

use super::{frame, native_losses, project, NATIVE_KIND};
use crate::feature::tests::{segment, test_header, test_label};
use crate::feature::{ClassId, PmDcWorkPlane, PmDcWorkPlanePayload, WORK_PLANE_TYPE};
use crate::record_identity::{Located, RecordTypeId};

fn finite(values: [f64; 3]) -> [FiniteReal; 3] {
    values.map(|value| FiniteReal::new(value).expect("finite"))
}

pub(crate) fn work_plane(
    ordinal: u32,
    origin: [f64; 3],
    u: [f64; 3],
    v: [f64; 3],
) -> PmDcWorkPlane {
    Located::new(
        PmDcWorkPlanePayload {
            save_version_major: 19,
            header: test_header(),
            entity_flags: 0x8000,
            values: vec![0; 3],
            marker: true,
            origin: finite(origin),
            u_axis: finite(u),
            v_axis: finite(v),
        },
        RecordTypeId::from_bytes(WORK_PLANE_TYPE),
        segment(),
        ordinal,
    )
}

#[test]
fn frame_converts_centimetres_and_takes_the_normal_from_u_cross_v() {
    let plane = work_plane(3, [0.0, 0.0, 1.5], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    let admitted = frame(&plane).expect("orthonormal frame");
    assert_eq!(admitted.origin().get(), Point3::new(0.0, 0.0, 15.0));
    assert_eq!(admitted.normal().get(), Vector3::new(0.0, 0.0, 1.0));
    assert_eq!(admitted.u_axis().get(), Vector3::new(1.0, 0.0, 0.0));
    let plane = work_plane(3, [2.0, -1.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]);
    let admitted = frame(&plane).expect("orthonormal frame");
    assert_eq!(admitted.origin().get(), Point3::new(20.0, -10.0, 0.0));
    assert_eq!(admitted.normal().get(), Vector3::new(1.0, 0.0, 0.0));
}

#[test]
fn frame_refuses_axes_that_are_not_orthonormal() {
    for (u, v) in [
        ([2.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        ([1.0, 0.0, 0.0], [0.0, 0.5, 0.0]),
        ([1.0, 0.0, 0.0], [0.6, 0.8, 0.0]),
        ([0.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        ([1.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
    ] {
        assert!(
            frame(&work_plane(3, [0.0; 3], u, v)).is_none(),
            "{u:?} {v:?}"
        );
    }
}

#[test]
fn projection_writes_a_named_datum_plane_or_a_native_work_plane() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("projection context");
    let label = test_label(3, 7, ClassId::try_from("0".repeat(32)).expect("class"), &[]);
    let plane = work_plane(3, [0.0, 0.0, 2.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    let feature = project(&ctx, &plane, Some(&label)).expect("projection");
    assert_eq!(feature.name.as_deref(), Some("Feature 7"));
    assert_eq!(feature.ordinal, 7);
    assert_eq!(feature.source_tag.as_deref(), Some("work_plane"));
    assert_eq!(feature.native_ref, Some(plane.id()));
    assert!(matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::DatumPlane { frame })
            if Some(*frame) == super::frame(&plane)
    ));
    assert!(native_losses(std::slice::from_ref(&plane), &[feature]).is_empty());

    let skewed = work_plane(3, [0.0; 3], [1.0, 0.0, 0.0], [0.6, 0.8, 0.0]);
    let feature = project(&ctx, &skewed, Some(&label)).expect("projection");
    assert_eq!(feature.source_tag.as_deref(), Some("native"));
    assert!(matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Native {
            kind: NativeFeatureKind::Other(kind),
            ..
        }) if kind == NATIVE_KIND
    ));
    let losses = native_losses(std::slice::from_ref(&skewed), &[feature]);
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        crate::loss::InventorLossCode::FeatureWorkPlaneFrameNative
            .note("test")
            .code
    );
    assert_eq!(
        losses[0].code.local_code(),
        "feature.work-plane-frame-native"
    );
}

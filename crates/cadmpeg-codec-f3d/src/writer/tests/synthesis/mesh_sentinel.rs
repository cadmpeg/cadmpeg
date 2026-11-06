// SPDX-License-Identifier: Apache-2.0
//! Source-less synthesis of a typed mesh-surface sentinel.

use std::io::Cursor;

use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder, NativeDisposition};
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::native_test::f3d_native;
use crate::test_support::smbh_geometry_test::synthetic_geometry_with_mesh_surface_smbh;
use crate::test_support::zip_test::f3d_with_smbh;
use crate::F3dCodec;

#[test]
fn typed_mesh_sentinel_survives_source_less_point_edit() {
    let source = f3d_with_smbh(&synthetic_geometry_with_mesh_surface_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut edited = decoded.ir().clone();
    let id = f3d_native(&edited).mesh_surface_sentinels[0].id();
    let point = &mut edited.model.points[0];
    let mut position = point.position().get();
    position.x += 1.0;
    point.set_position(cadmpeg_ir::features::FinitePoint3::new(position).unwrap());
    let plan = F3dCodec
        .plan(EncodeInput::new(&edited, None), TargetRequest::Inherit)
        .unwrap();
    assert_eq!(
        plan.report()
            .native_dispositions
            .record_disposition("f3d", "mesh_surface_sentinels", &id),
        NativeDisposition::Regenerated
    );
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(output.ir().model.bodies.len(), 1);
    assert_eq!(f3d_native(output.ir()).mesh_surface_sentinels.len(), 1);
    assert!(output
        .ir()
        .model
        .points
        .iter()
        .any(|point| point.position().get() == position));
}

#[test]
fn point_edit_with_mesh_sentinel_uses_patch() {
    let source = f3d_with_smbh(&synthetic_geometry_with_mesh_surface_smbh());
    let decoded = F3dCodec.decode(&mut Cursor::new(source), &DecodeOptions::default()).unwrap();
    let mut edited = decoded.ir().clone();
    let point = &mut edited.model.points[0];
    let mut position = point.position().get();
    position.x += 1.0;
    point.set_position(cadmpeg_ir::features::FinitePoint3::new(position).unwrap());
    let plan = F3dCodec.plan(EncodeInput::new(&edited, Some(decoded.source_fidelity())), TargetRequest::Inherit).unwrap();
    assert!(matches!(plan.report().write_path(), cadmpeg_ir::report::export::WritePath::Patched { .. }), "{:?}", plan.report().synthesis_cause());
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = F3dCodec.decode(&mut Cursor::new(bytes), &DecodeOptions::default()).unwrap();
    assert!((output.ir().model.points[0].position().get().x - position.x).abs() < 1e-12);
    crate::test_support::native_test::f3d_native_mut(&mut edited).unknowns.clear();
    let changed = F3dCodec.plan(EncodeInput::new(&edited, Some(decoded.source_fidelity())), TargetRequest::Inherit).unwrap();
    assert!(matches!(changed.report().write_path(), cadmpeg_ir::report::export::WritePath::Synthesized { .. }));
}

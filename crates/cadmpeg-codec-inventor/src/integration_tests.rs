// SPDX-License-Identifier: Apache-2.0

mod base;
mod kernel;
mod mirror;
mod motion;

use cadmpeg_container::compound::CompoundSnapshot;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::report::Severity;

use crate::rse::{RecordFrameState, RseInventory, SegmentBulkState};
use crate::test_support::test_fixtures::{
    acis_sphere_kernel_stream, primary_envelope_with_compression,
};
use crate::InventorCodec;

mod combine;
mod sketch_graph;
mod surface_extrusion;
mod thicken;

#[test]
fn rse_compression_methods_reach_records_and_valid_sphere_geometry() {
    for methods in [[1, 1], [1, 2], [2, 1], [2, 2]] {
        let bytes = primary_envelope_with_compression(&acis_sphere_kernel_stream(21_800), methods);
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("context");
        let compound = CompoundSnapshot::new(&ctx, root).expect("compound");
        let inventory = RseInventory::build(&ctx, &compound).expect("inventory");
        assert_eq!(inventory.segments.len(), 1);
        let SegmentBulkState::Framed(bulk) = &inventory.segments[0].bulk else {
            panic!("bulk must expand for {methods:?}")
        };
        let RecordFrameState::Framed(table) = &bulk.records else {
            panic!("records must frame for {methods:?}")
        };
        assert_eq!(table.records.len(), 1);

        let decoded = InventorCodec
            .decode(&mut std::io::Cursor::new(&bytes), &DecodeOptions::default())
            .expect("sphere decode");
        assert!(decoded.report().geometry_transferred());
        let (ir, report, fidelity) = decoded.into_parts();
        assert_eq!(ir.model.bodies.len(), 1);
        assert_eq!(ir.model.faces.len(), 1);
        assert_eq!(ir.model.surfaces.len(), 1);
        let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(sphere)) =
            &ir.model.surfaces[0].geometry
        else {
            panic!("fixture transfers a sphere")
        };
        assert!((sphere.radius().get() - 25.0).abs() <= 25.0 * f64::EPSILON);
        let mut check = cadmpeg_ir::validate::validate_neutral_with_source_fidelity(
            &ir,
            &fidelity,
            report.losses,
        )
        .expect("neutral and source-fidelity validation");
        check.findings.extend(
            InventorCodec
                .validate_native(&ctx, &ir, None)
                .expect("native validation"),
        );
        let errors: Vec<_> = check
            .findings
            .iter()
            .filter(|finding| finding.severity >= Severity::Error)
            .collect();
        assert!(errors.is_empty(), "{methods:?}: {errors:?}");
    }
}

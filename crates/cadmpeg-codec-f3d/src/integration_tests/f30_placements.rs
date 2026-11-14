// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::products::{Occurrence, OccurrenceParent};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::CadIr;

use super::{decode, f3d_native};
use crate::loss::F3dLossCode;

const EPS_F30_POSE: f64 = 1.0e-9;
const EPS_F30_FUSION_DISTANCE_MM: f64 = 1.0e-3;

fn world(ir: &CadIr, occurrence: &Occurrence) -> Transform {
    match &occurrence.parent {
        OccurrenceParent::Root {} => occurrence.transform,
        OccurrenceParent::Occurrence { occurrence: parent } => world(
            ir,
            ir.model
                .occurrences
                .iter()
                .find(|o| o.id == *parent)
                .unwrap(),
        )
        .compose(occurrence.transform)
        .unwrap(),
    }
}

#[test]
fn f30_insertions_follow_the_captured_owner_subtree_move() {
    let fixtures: [(&[u8], [f64; 3]); 2] = [
        (include_bytes!("../../../../corpus/f3d/F30a.f3z"), [0.0; 3]),
        (
            include_bytes!("../../../../corpus/f3d/F30b.f3z"),
            [20.0, 30.0, 50.0],
        ),
    ];
    for (bytes, movement) in fixtures {
        let result = decode(bytes.to_vec());
        let ir = result.ir();
        assert_eq!(ir.model.bodies.len(), 1);
        assert_eq!(ir.model.occurrences.len(), 5);
        for code in [
            F3dLossCode::XrefPlacementUndecoded,
            F3dLossCode::XrefPlacementIdentityAssumed,
        ] {
            assert!(result
                .report()
                .losses
                .iter()
                .all(|loss| loss.code != code.kind()));
        }
        let native = f3d_native(ir);
        assert_eq!(native.xref_references.len(), 2);
        let wire = serde_json::to_vec(ir).unwrap();
        let restored: CadIr = serde_json::from_slice(&wire).unwrap();
        assert_eq!(restored.model.occurrences, ir.model.occurrences);
        let mut poses = Vec::new();
        let mut world_vertices = Vec::new();
        for reference in &native.xref_references {
            let source = reference.placement_source.as_ref().unwrap();
            assert!([329, 337].contains(&source.carrier_record_index));
            assert!(crate::xref::placement_source_matches(reference).unwrap());
            let occurrence = ir
                .model
                .occurrences
                .iter()
                .find(|o| o.native_ref.as_deref() == Some(reference.id.as_str()))
                .unwrap();
            let local = occurrence.transform.rows();
            assert!((local[0][3] - 10.0).abs() < EPS_F30_POSE);
            let placement = world(ir, occurrence);
            poses.push(placement.rows());
            for vertex in &ir.model.vertices {
                let point = ir
                    .model
                    .points
                    .iter()
                    .find(|p| p.id == vertex.point)
                    .unwrap();
                world_vertices.push(placement.apply_point(point.position().get()).unwrap().get());
            }
        }
        if movement == [0.0; 3] {
            let mut longest_distance = 0.0_f64;
            for (index, a) in world_vertices.iter().enumerate() {
                for b in &world_vertices[index + 1..] {
                    let distance =
                        ((a.x - b.x).powi(2) + (a.y - b.y).powi(2) + (a.z - b.z).powi(2)).sqrt();
                    longest_distance = longest_distance.max(distance);
                }
            }
            // Fusion measured the F30a longest distance as 314.643 mm.
            assert!(
                (longest_distance - 314.643).abs() < EPS_F30_FUSION_DISTANCE_MM,
                "F30a maximum world vertex distance: {longest_distance} mm"
            );
        }
        poses.sort_by(|a, b| b[0][3].total_cmp(&a[0][3]));
        for (pose, (rotation, corner)) in poses
            .iter()
            .zip([(1.0, [10.0, 0.0, 0.0]), (-1.0, [-120.0, -130.0, 170.0])])
        {
            assert!((pose[0][0] - rotation).abs() < EPS_F30_POSE);
            assert!((pose[1][1] - rotation).abs() < EPS_F30_POSE);
            assert!((pose[2][2] - 1.0).abs() < EPS_F30_POSE);
            for axis in 0..3 {
                assert!((pose[axis][3] - corner[axis] - movement[axis]).abs() < EPS_F30_POSE);
            }
        }
        for display in &native.display_poses {
            let wire = serde_json::to_value(display).unwrap();
            assert_eq!(wire["binding"]["state"], "bound");
        }
        let validation = cadmpeg_ir::validate_neutral(ir, result.report().losses.clone()).unwrap();
        assert!(validation.is_ok(), "{validation:#?}");
    }
}

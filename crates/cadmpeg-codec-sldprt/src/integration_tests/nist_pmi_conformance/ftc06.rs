use std::collections::BTreeSet;

use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use cadmpeg_ir::pmi::{PmiDefinition, PmiTarget};

use super::specimen;

#[test]
fn ftc_06_emits_base_datums_with_face_targets() {
    let result = specimen("nist_ftc_06_asme1_rd_sw1802.SLDPRT");
    let ir = result.ir();
    let expected = [
        ("G", 1, "cylinder"),
        ("H", 1, "cylinder"),
        ("J", 2, "plane"),
        ("K", 2, "plane"),
    ];

    for (identification, target_count, surface_kind) in expected {
        let annotations = ir
            .model
            .pmi
            .iter()
            .filter(|annotation| {
                matches!(
                    &annotation.definition,
                    PmiDefinition::Datum { identification: actual }
                        if actual == identification
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            annotations.len(),
            1,
            "expected one datum {identification} annotation"
        );
        let annotation = annotations.first().expect("datum annotation");
        let faces = annotation
            .targets
            .iter()
            .filter_map(|target| match target {
                PmiTarget::Face { face } => Some(face),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(annotation.targets.len(), target_count);
        assert_eq!(faces.len(), target_count, "datum {identification} faces");

        for face_id in faces {
            let face = ir
                .model
                .faces
                .iter()
                .find(|face| &face.id == face_id)
                .expect("datum target face");
            let surface = ir
                .model
                .surfaces
                .iter()
                .find(|surface| surface.id == face.surface)
                .expect("datum target surface");
            let actual_kind = match surface.geometry.solved() {
                Some(SolvedSurfaceGeometry::Cylinder(_)) => "cylinder",
                Some(SolvedSurfaceGeometry::Plane(_)) => "plane",
                _ => "other",
            };
            assert_eq!(
                actual_kind, surface_kind,
                "datum {identification} target surface"
            );
        }
    }
}

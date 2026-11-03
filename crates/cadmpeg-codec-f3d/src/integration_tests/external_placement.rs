// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::loss::F3dLossCode;
use crate::test_support::assembly_test::{f3d_with_identity_xrefs, f3d_without_brep};

#[test]
fn admitted_identity_merges_and_unreadable_placement_omits_member_subtree() {
    for admitted in [false, true] {
        let targets = [("child.f3d", XREF_ROLE)];
        let root = if admitted {
            f3d_with_identity_xrefs("assembly-design", "root.f3d", &targets)
        } else {
            f3d_without_brep("assembly-design", "root.f3d", &targets)
        };
        let child =
            f3d_with_identity_xrefs("assembly-design", "child.f3d", &[("leaf.f3d", XREF_ROLE)]);
        let leaf = f3d_with_smbh(&synthetic_geometry_smbh());
        let archive = f3z_archive(
            "root.f3d",
            &[
                ("root.f3d", &root),
                ("child.f3d", &child),
                ("leaf.f3d", &leaf),
            ],
        );
        let result = decode(archive.clone());
        assert_eq!(
            result.ir().model.bodies.len(),
            usize::from(admitted),
            "admitted={admitted} {:?}",
            result.report()
        );
        assert_eq!(
            result.ir().model.occurrences.len(),
            2 * usize::from(admitted)
        );
        assert!(result.ir().model.assembly_joints.is_empty());
        let native = f3d_native(result.ir());
        assert_eq!(native.xref_references.len(), 1 + usize::from(admitted));
        let reference = &native.xref_references[0];
        assert_eq!(reference.placement_source.is_some(), admitted);
        assert!(reference.transform.is_none());
        assert_eq!(
            result
                .report()
                .losses
                .iter()
                .filter(|loss| { loss.code == F3dLossCode::XrefOccurrenceUnplaced.kind() })
                .count(),
            usize::from(!admitted)
        );
        if admitted {
            assert!(result.ir().model.occurrences.iter().all(|occurrence| {
                occurrence.transform == cadmpeg_ir::transform::Transform::identity()
            }));
        } else {
            assert_eq!(
                result
                    .source_fidelity()
                    .retained_record(crate::ids::FILE_SOURCE_IMAGE_ID)
                    .and_then(|record| record.data()),
                Some(archive.as_slice())
            );
            let loss = result
                .report()
                .losses
                .iter()
                .find(|loss| loss.code == F3dLossCode::XrefOccurrenceUnplaced.kind())
                .unwrap();
            assert!(loss.message.contains("child.f3d"));
            assert!(result
                .report()
                .notes
                .iter()
                .all(|note| !note.contains(": merged ")));
        }
        assert_valid(&result);
    }
}

#[test]
fn unadmitted_references_report_one_omission_each() {
    let result = decode(f3d_without_brep(
        "assembly-design",
        "root.f3d",
        &[("first.f3d", "role"), ("second.f3d", "role")],
    ));
    assert!(result.ir().model.occurrences.is_empty());
    assert!(result.ir().model.bodies.is_empty());
    assert!(result.ir().model.assembly_joints.is_empty());
    assert_eq!(f3d_native(result.ir()).xref_references.len(), 2);
    assert_eq!(
        result
            .report()
            .losses
            .iter()
            .filter(|loss| { loss.code == F3dLossCode::XrefOccurrenceUnplaced.kind() })
            .count(),
        2
    );
    assert_valid(&result);
}

// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use cadmpeg_ir::ids::FaceId;
use cadmpeg_ir::pmi::PmiTarget;

use super::{dimension_targets_by_slot, DimensionTarget};

fn face(id: &str) -> PmiTarget {
    PmiTarget::Face {
        face: FaceId::try_from(format!("creo:visibgeom:face#{id}")).expect("face id"),
    }
}

fn target(faces: &[&str]) -> DimensionTarget {
    DimensionTarget {
        faces: faces.iter().map(|id| face(id)).collect(),
        from_edges: false,
    }
}

#[test]
fn slot_targets_need_agreement_across_sections() {
    let targets = BTreeMap::from([
        (("BasFullData", 417), target(&["506"])),
        (("FullMData", 417), target(&["506"])),
        (("BasFullData", 418), target(&["1"])),
        (("FullMData", 418), target(&["2"])),
    ]);
    let by_slot = dimension_targets_by_slot(&targets);
    assert_eq!(by_slot.get(&417), Some(&vec![face("506")]));
    assert_eq!(by_slot.get(&418), None);
}

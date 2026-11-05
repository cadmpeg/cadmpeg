// SPDX-License-Identifier: Apache-2.0
//! IGES B-rep loop ordering and orientations.

use cadmpeg_ir::topology::{PcurveUse, Sense};

/// The Type 508 edge-use orientation of a coedge. A Type 508 loop keeps its
/// face on the left about the support normal, and a reversed face keeps it on
/// the left about the reversed normal, so a reversed face writes its loops
/// backward.
pub(super) fn edge_use_sense(coedge: Sense, face: Sense) -> Sense {
    if coedge == face {
        Sense::Forward
    } else {
        Sense::Reversed
    }
}

/// The pcurve uses of a pole-vertex use in Type 508 order. A reversed face
/// writes its loops backward, so it writes the uses in reverse order.
pub(super) fn pole_use_order(uses: &[PcurveUse], face: Sense) -> Vec<&PcurveUse> {
    match face {
        Sense::Forward => uses.iter().collect(),
        Sense::Reversed => uses.iter().rev().collect(),
    }
}

/// The ring positions of a loop in Type 508 edge-use order.
pub(super) fn edge_use_positions(count: usize, face: Sense) -> Vec<usize> {
    match face {
        Sense::Forward => (0..count).collect(),
        Sense::Reversed => (0..count).rev().collect(),
    }
}

pub(super) fn brep_sense(sense: Sense) -> i32 {
    match sense {
        Sense::Forward => 1,
        Sense::Reversed => 0,
    }
}

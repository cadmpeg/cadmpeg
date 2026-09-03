// SPDX-License-Identifier: Apache-2.0
//! Frozen accept/reject candidates for Phase 5 gate swaps.
//!
//! These fixtures pin outcomes under the current full validator before any
//! production gate switches onto a narrow Check subset. Both accepted and
//! rejected sides are required: the rollback paths are driven by rejections.

use crate::ids::{PointId, RegionId, ShellId, VertexId};
use crate::topology::{Shell, Vertex};
use crate::CadIr;

/// Empty document: accepted by every current production gate.
pub fn accepted_empty() -> CadIr {
    CadIr::empty()
}

/// Vertex → missing point: rejected (`ReferentialIntegrity`).
pub fn rejected_missing_point(prefix: &str) -> CadIr {
    let mut ir = CadIr::empty();
    ir.model.vertices.push(Vertex {
        id: VertexId(format!("{prefix}:vertex#0")),
        point: PointId(format!("{prefix}:point#missing")),
        tolerance: None,
    });
    ir
}

/// Shell → missing region: rejected (`ReferentialIntegrity` / topology).
pub fn rejected_missing_region(prefix: &str) -> CadIr {
    let mut ir = CadIr::empty();
    ir.model.shells.push(Shell {
        id: ShellId(format!("{prefix}:shell#0")),
        region: RegionId(format!("{prefix}:region#missing")),
        faces: Vec::new(),
        wire_edges: Vec::new(),
        free_vertices: Vec::new(),
    });
    ir
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::annotations::Annotations;
    use crate::report::Check;
    use crate::validate::{validate_neutral, validate_neutral_with_annotations};

    /// Rhino draft gate today: full annotations validation minus `ArenaOrder`.
    fn rhino_draft_gate(ir: &CadIr, annotations: &Annotations) -> bool {
        let mut validation = validate_neutral_with_annotations(ir, annotations, Vec::new());
        validation
            .findings
            .retain(|finding| finding.check != Check::ArenaOrder);
        validation.is_ok()
    }

    /// Rhino instance gate today: full neutral validation.
    fn rhino_instance_gate(ir: &CadIr) -> bool {
        validate_neutral(ir, Vec::new()).is_ok()
    }

    #[test]
    fn freeze_accepted_empty_under_current_gates() {
        let ir = accepted_empty();
        let annotations = Annotations::default();
        assert!(validate_neutral(&ir, Vec::new()).is_ok());
        assert!(rhino_draft_gate(&ir, &annotations));
        assert!(rhino_instance_gate(&ir));
    }

    #[test]
    fn freeze_rejected_missing_point_under_current_gates() {
        let ir = rejected_missing_point("test:model");
        let annotations = Annotations::default();
        let report = validate_neutral(&ir, Vec::new());
        assert!(!report.is_ok(), "{report:?}");
        assert!(report
            .findings
            .iter()
            .any(|f| f.check == Check::ReferentialIntegrity));
        assert!(!rhino_draft_gate(&ir, &annotations));
        assert!(!rhino_instance_gate(&ir));
    }

    #[test]
    fn freeze_rejected_missing_region_under_current_gates() {
        let ir = rejected_missing_region("test:model");
        let annotations = Annotations::default();
        assert!(!validate_neutral(&ir, Vec::new()).is_ok());
        assert!(!rhino_draft_gate(&ir, &annotations));
        assert!(!rhino_instance_gate(&ir));
    }
}

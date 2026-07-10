// SPDX-License-Identifier: Apache-2.0
//! Source-format namespaces retained outside the format-neutral model.

mod f3d;
mod sldprt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub use f3d::{F3dNative, F3D_NATIVE_VERSION};
pub use sldprt::{SldprtNative, SLDPRT_NATIVE_VERSION};

/// One non-empty native arena reported as an exporter loss.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LossCount {
    /// Source-format namespace this arena belongs to (e.g. `"f3d"`, `"sldprt"`).
    pub format: String,
    /// Arena field name within that namespace (e.g. `"sketch_points"`).
    pub kind: String,
    /// Number of records in the arena.
    pub count: usize,
}

/// Native records grouped by independently versioned source-format namespace.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Native {
    /// Fusion `.f3d` native arenas, present only when the document was decoded
    /// from an `.f3d` source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub f3d: Option<F3dNative>,
    /// `SolidWorks` `.sldprt` native arenas, present only when the document was
    /// decoded from an `.sldprt` source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sldprt: Option<SldprtNative>,
}

impl Native {
    /// Return one count for each non-empty native arena.
    pub fn loss_counts(&self) -> Vec<LossCount> {
        let mut counts = Vec::new();

        if let Some(native) = &self.f3d {
            push_count(
                &mut counts,
                "f3d",
                "act_entities",
                native.act_entities.len(),
            );
            push_count(&mut counts, "f3d", "act_guids", native.act_guids.len());
            push_count(
                &mut counts,
                "f3d",
                "act_root_components",
                native.act_root_components.len(),
            );
            push_count(
                &mut counts,
                "f3d",
                "design_objects",
                native.design_objects.len(),
            );
            push_count(
                &mut counts,
                "f3d",
                "design_entity_headers",
                native.design_entity_headers.len(),
            );
            push_count(
                &mut counts,
                "f3d",
                "design_record_headers",
                native.design_record_headers.len(),
            );
            push_count(
                &mut counts,
                "f3d",
                "design_body_members",
                native.design_body_members.len(),
            );
            push_count(
                &mut counts,
                "f3d",
                "construction_recipes",
                native.construction_recipes.len(),
            );
            push_count(
                &mut counts,
                "f3d",
                "persistent_design_links",
                native.persistent_design_links.len(),
            );
            push_count(
                &mut counts,
                "f3d",
                "persistent_references",
                native.persistent_references.len(),
            );
            push_count(
                &mut counts,
                "f3d",
                "sketch_curve_links",
                native.sketch_curve_links.len(),
            );
            push_count(
                &mut counts,
                "f3d",
                "sketch_relations",
                native.sketch_relations.len(),
            );
            push_count(
                &mut counts,
                "f3d",
                "sketch_points",
                native.sketch_points.len(),
            );
            push_count(
                &mut counts,
                "f3d",
                "sketch_curve_identities",
                native.sketch_curve_identities.len(),
            );
            push_count(
                &mut counts,
                "f3d",
                "lost_edge_references",
                native.lost_edge_references.len(),
            );
            push_count(
                &mut counts,
                "f3d",
                "asm_histories",
                native.asm_histories.len(),
            );
        }

        if let Some(native) = &self.sldprt {
            push_count(
                &mut counts,
                "sldprt",
                "feature_histories",
                native.feature_histories.len(),
            );
            push_count(
                &mut counts,
                "sldprt",
                "feature_input_lanes",
                native.feature_input_lanes.len(),
            );
        }

        counts
    }
}

fn push_count(counts: &mut Vec<LossCount>, format: &str, kind: &str, count: usize) {
    if count != 0 {
        counts.push(LossCount {
            format: format.to_owned(),
            kind: kind.to_owned(),
            count,
        });
    }
}

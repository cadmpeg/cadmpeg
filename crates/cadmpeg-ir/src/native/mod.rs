// SPDX-License-Identifier: Apache-2.0
//! Source-format namespaces retained outside the format-neutral model.

pub(crate) mod f3d;
mod sldprt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub use f3d::{F3dNative, F3D_NATIVE_VERSION};
pub use sldprt::{SldprtNative, SLDPRT_NATIVE_VERSION};

/// One non-empty native arena reported as an exporter loss.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LossCount {
    /// Source-format namespace this arena belongs to.
    pub format: String,
    /// Arena field name within that namespace.
    pub kind: String,
    /// Number of records in the arena.
    pub count: usize,
}

/// Native records grouped by independently versioned source-format namespace.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Native {
    /// Fusion `.f3d` native arenas.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub f3d: Option<F3dNative>,
    /// `SolidWorks` `.sldprt` native arenas.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sldprt: Option<SldprtNative>,
}

impl Native {
    /// Sort every present native namespace into canonical identity order.
    pub(crate) fn finalize(&mut self) {
        if let Some(native) = &mut self.f3d {
            native.finalize();
        }
        if let Some(native) = &mut self.sldprt {
            native.finalize();
        }
    }

    /// Return one count for each non-empty native arena.
    pub fn loss_counts(&self) -> Vec<LossCount> {
        let mut counts = Vec::new();
        if let Some(native) = &self.f3d {
            counts.extend(loss_counts("f3d", native.loss_counts()));
        }
        if let Some(native) = &self.sldprt {
            counts.extend(loss_counts("sldprt", native.loss_counts()));
        }
        counts
    }
}

fn loss_counts(
    format: &'static str,
    counts: Vec<(&'static str, usize)>,
) -> impl Iterator<Item = LossCount> {
    counts.into_iter().map(move |(kind, count)| LossCount {
        format: format.to_owned(),
        kind: kind.to_owned(),
        count,
    })
}

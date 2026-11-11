// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::report::loss::LossNote;

use super::index::IndexedCurve;
use crate::loss::SldprtLossCode;

#[derive(Default)]
struct Count {
    count: usize,
    largest: f64,
}

impl Count {
    fn add(&mut self, tolerance: f64) {
        self.count += 1;
        self.largest = self.largest.max(tolerance);
    }
}

#[derive(Default)]
pub(crate) struct ApproximationCounts {
    intersection: Count,
    surface_curve: Count,
    blend: Count,
    inverse_boundary: Count,
    absent_tolerance: usize,
}

impl ApproximationCounts {
    pub(super) fn intersection(&mut self, curve: &IndexedCurve) {
        if let IndexedCurve::Derived(chart) = curve {
            match chart.support_data.fit_tolerance_mm {
                Some(tolerance) => self.intersection.add(tolerance),
                None => self.absent_tolerance += 1,
            }
        }
    }

    pub(super) fn surface_curve(&mut self, tolerance: f64) {
        self.surface_curve.add(tolerance);
    }

    pub(super) fn blend(&mut self, tolerance: f64) {
        self.blend.add(tolerance);
    }

    pub(super) fn inverse_boundary(&mut self, distance: f64) {
        self.inverse_boundary.add(distance);
    }

    pub(super) fn take_notes(&mut self) -> Vec<LossNote> {
        let counts = std::mem::take(self);
        let mut notes = Vec::new();
        for (count, kind, size) in [
            (
                counts.intersection,
                "intersection chart curve(s)",
                "fit tolerance",
            ),
            (
                counts.surface_curve,
                "fitted surface-parameter curve(s)",
                "sampled fit tolerance",
            ),
            (counts.blend, "fitted blend surface(s)", "fit tolerance"),
            (
                counts.inverse_boundary,
                "inverse NURBS boundary cache(s)",
                "sampled maximum distance",
            ),
        ] {
            if count.count > 0 {
                notes.push(SldprtLossCode::GeometryCarrierApproximated.note(format!(
                    "{} {kind} use approximate carriers; largest {size} {} mm.",
                    count.count, count.largest,
                )));
            }
        }
        if counts.absent_tolerance > 0 {
            notes.push(SldprtLossCode::GeometryCarrierApproximated.note(format!(
                "{} intersection chart curve(s) use approximate carriers with absent source tolerance; largest fit tolerance unavailable.",
                counts.absent_tolerance,
            )));
        }
        notes
    }
}

#[cfg(test)]
mod tests;

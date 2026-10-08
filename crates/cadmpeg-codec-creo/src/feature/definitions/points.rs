// SPDX-License-Identifier: Apache-2.0
//! Reconciliation of section coordinates by point identity.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use super::{FeatureVariableTable, ReconciledPoints, VariableType, EPS_PARAMETER_AGREEMENT, TRIM_COORDINATE_EPS};

#[derive(Clone, Copy)]
struct CoordinateAgreement {
    first: f64,
    scale: f64,
    difference: f64,
    undefined_difference: bool,
}

impl CoordinateAgreement {
    fn new(first: f64) -> Self {
        Self {
            first,
            scale: first.abs().max(1.0),
            difference: 0.0,
            undefined_difference: !first.is_finite(),
        }
    }

    fn add(&mut self, value: f64) {
        self.scale = self.scale.max(value.abs());
        let difference = (value - self.first).abs();
        self.undefined_difference |= difference.is_nan();
        self.difference = self.difference.max(difference);
    }

    fn agrees(self) -> bool {
        !self.undefined_difference && self.difference <= EPS_PARAMETER_AGREEMENT * self.scale
    }
}

#[derive(Default)]
struct RadiusAgreement {
    first: Option<cadmpeg_ir::scalar::FiniteReal>,
    missing: bool,
    conflict: bool,
}

impl RadiusAgreement {
    fn add(&mut self, value: Option<f64>) {
        let Some(value) = value else {
            self.missing = true;
            return;
        };
        let Some(value) = cadmpeg_ir::scalar::FiniteReal::new(value) else {
            self.conflict = true;
            return;
        };
        if let Some(first) = self.first {
            let scale = first.get().abs().max(value.get().abs()).max(1.0);
            self.conflict |= (value.get() - first.get()).abs() > TRIM_COORDINATE_EPS * scale;
        } else {
            self.first = Some(value);
        }
    }

    fn value(&self) -> Result<Option<cadmpeg_ir::scalar::FiniteReal>, ()> {
        if self.conflict || (self.missing && self.first.is_some()) {
            Err(())
        } else {
            Ok(self.first)
        }
    }
}

pub(super) struct TrimGeometry {
    pub(super) coordinates: ReconciledPoints<[Option<f64>; 2]>,
    radii: HashMap<u32, RadiusAgreement>,
}

impl TrimGeometry {
    pub(super) fn radius(&self, key: u32) -> Result<Option<cadmpeg_ir::scalar::FiniteReal>, ()> {
        self.radii.get(&key).map_or(Ok(None), RadiusAgreement::value)
    }
}

impl FeatureVariableTable {
    /// Reconcile repeated and complementary section-point rows by identity.
    pub(crate) fn reconciled_points(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<ReconciledPoints<[Option<f64>; 2]>, CodecError> {
        Ok(self.reconciled_geometry(ctx, false)?.coordinates)
    }

    pub(super) fn reconciled_trim_geometry(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<TrimGeometry, CodecError> {
        self.reconciled_geometry(ctx, true)
    }

    fn reconciled_geometry(
        &self,
        ctx: &DecodeContext<'_>,
        include_radii: bool,
    ) -> Result<TrimGeometry, CodecError> {
        let mut radii = HashMap::<u32, RadiusAgreement>::new();
        let mut storage = ctx.reserve_scoped(0, "creo point coordinate groups")?;
        let mut coordinates = BTreeMap::<u32, [Option<CoordinateAgreement>; 2]>::new();
        for row in ctx.admit_iter(&self.rows, "creo point variable traversal")? {
            if include_radii && row.variable_type == VariableType::Radius {
                ctx.entry_hash_map(&mut radii, row.key, "creo trim radius groups")?
                    .or_default().add(row.value.value());
            }
            let coordinate = match row.variable_type {
                VariableType::U => 0,
                VariableType::V => 1,
                _ => continue,
            };
            let point = storage.with_storage(|| ctx.entry_btree_map(
                &mut coordinates, row.key, "creo reconciled point ID nodes"))?
                .or_default();
            if let Some(value) = row.value.value() {
                match &mut point[coordinate] {
                    Some(agreement) => agreement.add(value),
                    slot @ None => *slot = Some(CoordinateAgreement::new(value)),
                }
            }
        }
        let mut points = BTreeMap::new();
        let mut ambiguous = BTreeSet::new();
        for (point_id, coordinates) in ctx.admit_iter(coordinates, "creo point group traversal")? {
            let mut point = [None; 2];
            let mut conflict = false;
            for (coordinate, agreement) in coordinates.into_iter().enumerate() {
                if let Some(agreement) = agreement {
                    if agreement.agrees() {
                        point[coordinate] = Some(agreement.first);
                    } else {
                        conflict = true;
                    }
                }
            }
            if conflict {
                ctx.insert_btree_set(&mut ambiguous, point_id, "creo ambiguous point nodes")?;
            } else {
                ctx.insert_btree_map(&mut points, point_id, point, "creo reconciled point nodes")?;
            }
        }
        Ok(TrimGeometry { coordinates: ReconciledPoints { points, ambiguous }, radii })
    }
}
#[cfg(test)]
mod tests {
    use super::CoordinateAgreement;

    #[test]
    fn coordinate_agreement_preserves_nonfinite_scale_rules() {
        let agrees = |values: &[f64]| {
            let mut agreement = CoordinateAgreement::new(values[0]);
            for &value in &values[1..] { agreement.add(value); }
            agreement.agrees()
        };
        assert!(agrees(&[0.0, -0.0]));
        assert!(agrees(&[1.0, 1.0 + super::EPS_PARAMETER_AGREEMENT / 2.0]));
        assert!(!agrees(&[1.0, 2.0]));
        assert!(agrees(&[1.0, 2.0, f64::INFINITY]));
        assert!(!agrees(&[f64::INFINITY]));
        assert!(!agrees(&[f64::NAN]));
        assert!(!agrees(&[1.0, f64::NAN, f64::INFINITY]));
        assert!(!agrees(&[f64::MAX, -f64::MAX]));
    }
}

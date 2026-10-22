// SPDX-License-Identifier: Apache-2.0
//! Body volume, centroid, closure and orientation from face fluxes.
//!
//! A body sums its faces' fluxes with the sign `σ` of each face's oriented
//! normal reading, times the sign of the body placement's determinant. The
//! volume and centroid are measured only when every shell is closed, every
//! face has flux, every face's orientation is established, the closure flux
//! vanishes within its tolerance, and the outer shell's volume is positive
//! and each void's negative. Otherwise they are unavailable with the first
//! failing reason and the readings that remain computable as candidates.

use cadmpeg_ir::math::Point3;

use crate::face::{FaceFlux, FaceResult, Verdict};
use crate::failure::Missing;
use crate::flux::Flux;
use crate::reason::{Category, UnavailableReason};
use crate::report::{BodyOrientation, ClosureFlux, NormalReading, RegionRule};
use crate::value::{
    measured, Candidate, Errors, Method, Point, Quantity, Reading, Scalar, Unavailable, Unit,
};

/// One orientation reading of a body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BodyReading {
    /// Every face's sense, declared regions.
    FaceSense,
    /// Loop winding where determinable, witnessed regions.
    LoopWinding,
    /// Face sense with every unconfirmed declared-orientation face in its
    /// complement reading.
    Complement,
}

impl BodyReading {
    const fn reading(self) -> Reading {
        match self {
            Self::FaceSense => Reading::FaceSense,
            Self::LoopWinding => Reading::LoopWinding,
            Self::Complement => Reading::Complement,
        }
    }
}

/// One face of a body with its result.
#[derive(Clone, Copy)]
pub(super) struct Member<'a> {
    pub(super) id: &'a str,
    pub(super) shell: usize,
    pub(super) result: &'a FaceResult,
}

fn sign(reading: NormalReading) -> Option<f64> {
    match reading {
        NormalReading::Surface => Some(1.0),
        NormalReading::Reversed => Some(-1.0),
        NormalReading::Mixed | NormalReading::NotDeterminable => None,
    }
}

/// The body's fluxes under one reading: the sum and each shell's volume.
#[derive(Debug, Clone)]
struct Sum {
    flux: Flux,
    shells: Vec<f64>,
}

/// What a body's volume reads.
pub(super) struct VolumeInput<'a> {
    pub(super) id: &'a str,
    pub(super) members: Vec<Member<'a>>,
    /// Per shell in region order, whether it is closed and whether it is the
    /// outer shell.
    pub(super) shells: Vec<(bool, bool)>,
    pub(super) origin: Point3,
    /// `−1` under a reflecting body placement.
    pub(super) handedness: f64,
    /// The body's area, its largest boundary tolerance and box diagonal.
    pub(super) area: f64,
    pub(super) diagonal: f64,
    /// The placement term of a value of dimension `d`.
    pub(super) placement: &'a dyn Fn(u8, f64) -> f64,
}

/// The body's volume, centroid, area centroid and orientation facts.
pub(super) struct VolumeFacts {
    pub(super) volume: Quantity<Scalar>,
    pub(super) centroid: Quantity<Point>,
    pub(super) area_centroid: Option<Quantity<Point>>,
    pub(super) shell_volumes: Vec<Option<Quantity<Scalar>>>,
    pub(super) orientation: BodyOrientation,
}

impl VolumeInput<'_> {
    /// The flux and sign a face contributes under `reading`, or why it has
    /// none.
    fn term(&self, member: &Member<'_>, reading: BodyReading) -> Result<(Flux, f64), Missing> {
        let result = member.result;
        let missing = || Missing {
            reason: UnavailableReason::AggregateMemberUnavailable,
            entity: Some(member.id.to_owned()),
            message: format!("member {} has no flux", member.id),
        };
        let flux: &FaceFlux = match &result.flux {
            Some(Ok(flux)) => flux,
            Some(Err(missing)) => return Err(missing.clone()),
            None => return Err(missing()),
        };
        let orientation = &result.row.orientation;
        let declared = sign(orientation.declared_normal).unwrap_or(1.0) * self.handedness;
        let complement = |sigma: f64| -> Result<(Flux, f64), Missing> {
            flux.complement
                .map(|complement| (complement, -sigma))
                .ok_or_else(missing)
        };
        match reading {
            BodyReading::FaceSense => Ok((flux.declared, declared)),
            BodyReading::LoopWinding => match result.verdict {
                Verdict::Conflict => complement(declared),
                Verdict::Confirmed | Verdict::Unconfirmed => Ok((flux.declared, declared)),
                Verdict::Settled => Ok((
                    flux.declared,
                    sign(orientation.loop_normal).map_or(declared, |loops| loops * self.handedness),
                )),
            },
            BodyReading::Complement => match result.verdict {
                Verdict::Unconfirmed => complement(declared),
                _ => Ok((flux.declared, declared)),
            },
        }
    }

    fn sum(&self, reading: BodyReading) -> Result<Sum, Missing> {
        let mut sum = Sum {
            flux: Flux::ZERO,
            shells: vec![0.0; self.shells.len()],
        };
        for member in &self.members {
            let (flux, sigma) = self.term(member, reading)?;
            sum.flux = sum.flux.add_oriented(flux, sigma);
            if let Some(shell) = sum.shells.get_mut(member.shell) {
                *shell += sigma * flux.volume();
            }
        }
        Ok(sum)
    }

    fn volume_errors(&self, flux: &Flux, value: f64) -> Errors {
        let tau = self
            .members
            .iter()
            .map(|member| member.result.tau_b)
            .fold(0.0, f64::max);
        Errors::new(flux.error[0], self.area * tau + (self.placement)(3, value))
    }

    fn volume_quantity(&self, sum: &Sum) -> Quantity<Scalar> {
        let value = sum.flux.volume();
        measured(
            Scalar::new(value),
            Unit::CubicMillimeter,
            Method::DivergenceQuadrature,
            self.volume_errors(&sum.flux, value),
            self.id,
        )
    }

    fn centroid_quantity(&self, sum: &Sum) -> Quantity<Point> {
        let volume = sum.flux.volume();
        let moment = sum.flux.moment();
        let errors = self.volume_errors(&sum.flux, volume);
        let point = Point3::new(
            self.origin.x + moment[0] / volume,
            self.origin.y + moment[1] / volume,
            self.origin.z + moment[2] / volume,
        );
        let moment_error = sum.flux.error[1]
            .max(sum.flux.error[2])
            .max(sum.flux.error[3]);
        let lever = moment.iter().fold(0.0_f64, |most, m| most.max(m.abs()));
        measured(
            Point::new(point),
            Unit::Millimeter,
            Method::DivergenceQuadrature,
            Errors::new(
                moment_error / volume.abs() + lever * errors.numeric / (volume * volume),
                errors.model * self.diagonal / volume.abs(),
            ),
            self.id,
        )
    }

    /// `|Z|` and its tolerance under one reading.
    fn closure(&self, sum: &Sum) -> (f64, f64, Errors) {
        let [x, y, z] = sum.flux.closure();
        let magnitude = (x * x + y * y + z * z).sqrt();
        let numeric = sum.flux.error[4] + sum.flux.error[5] + sum.flux.error[6];
        let model: f64 = self
            .members
            .iter()
            .map(|member| member.result.boundary_length * member.result.tau_b)
            .sum();
        (magnitude, numeric + model, Errors::new(numeric, model))
    }

    fn closure_quantity(&self, reading: BodyReading) -> Quantity<Scalar> {
        match self.sum(reading) {
            Ok(sum) => {
                let (magnitude, _, errors) = self.closure(&sum);
                measured(
                    Scalar::new(magnitude),
                    Unit::SquareMillimeter,
                    Method::DivergenceQuadrature,
                    errors,
                    self.id,
                )
            }
            Err(missing) => missing.quantity(),
        }
    }

    /// The first failing precondition of a measured volume, with its
    /// category for an aggregate.
    fn failure(
        &self,
        sum: &Result<Sum, Missing>,
    ) -> Option<(UnavailableReason, String, Option<Category>)> {
        if self.shells.iter().any(|(closed, _)| !closed) {
            return Some((
                UnavailableReason::VolumeShellOpen,
                "a shell of the body is open".to_owned(),
                None,
            ));
        }
        let sum = match sum {
            Ok(sum) => sum,
            Err(missing) => {
                let category = missing.reason.category().unwrap_or(Category::Input);
                return Some((
                    UnavailableReason::AggregateMemberUnavailable,
                    format!(
                        "member {} has no flux: {}",
                        missing.entity.as_deref().unwrap_or("?"),
                        missing.reason.code()
                    ),
                    Some(category),
                ));
            }
        };
        let mut conflict = Vec::new();
        let mut unverified = Vec::new();
        for member in &self.members {
            let result = member.result;
            match (
                result.verdict,
                result.row.measures.region,
                result.row.orientation.consistent,
            ) {
                (Verdict::Conflict, ..) | (_, _, Some(false)) => conflict.push(member.id),
                (Verdict::Unconfirmed, ..) => unverified.push(member.id),
                (_, Some(RegionRule::WholeSurface), _) | (_, _, Some(true)) => {}
                (_, _, None) => unverified.push(member.id),
            }
        }
        if !conflict.is_empty() {
            return Some((
                UnavailableReason::VolumeOrientationConflict,
                format!(
                    "{} face(s) disagree with their readings: {}",
                    conflict.len(),
                    conflict.join(", ")
                ),
                None,
            ));
        }
        if !unverified.is_empty() {
            return Some((
                UnavailableReason::VolumeOrientationUnverified,
                format!(
                    "{} face(s) rest on an unconfirmed orientation: {}",
                    unverified.len(),
                    unverified.join(", ")
                ),
                None,
            ));
        }
        let (magnitude, tolerance, _) = self.closure(sum);
        if magnitude > tolerance {
            return Some((
                UnavailableReason::VolumeClosureFlux,
                format!("the closure flux {magnitude} mm² exceeds its tolerance {tolerance} mm²"),
                None,
            ));
        }
        for (position, ((_, outer), volume)) in self.shells.iter().zip(&sum.shells).enumerate() {
            let wrong = if *outer {
                *volume <= 0.0
            } else {
                *volume >= 0.0
            };
            if wrong {
                return Some((
                    UnavailableReason::VolumeOrientationConflict,
                    format!("shell {position} has a volume of the wrong sign for its role"),
                    None,
                ));
            }
        }
        None
    }

    /// The candidates of an unavailable volume and centroid.
    fn candidates(
        &self,
        reason: UnavailableReason,
    ) -> (Vec<Candidate<Scalar>>, Vec<Candidate<Point>>) {
        let mut readings = vec![BodyReading::FaceSense, BodyReading::LoopWinding];
        if reason == UnavailableReason::VolumeOrientationUnverified {
            readings.push(BodyReading::Complement);
        }
        let mut volumes = Vec::new();
        let mut centroids = Vec::new();
        for reading in readings {
            let Ok(sum) = self.sum(reading) else {
                continue;
            };
            if let Quantity::Measured(volume) = self.volume_quantity(&sum) {
                volumes.push(volume.into_candidate(reading.reading()));
            }
            if let Quantity::Measured(centroid) = self.centroid_quantity(&sum) {
                centroids.push(centroid.into_candidate(reading.reading()));
            }
        }
        (volumes, centroids)
    }

    /// The body's volume, centroid, area centroid and orientation facts.
    pub(super) fn measure(&self) -> VolumeFacts {
        let in_use = self.sum(BodyReading::FaceSense);
        let orientation = self.orientation();
        let area_centroid = self.area_centroid(&in_use);
        match self.failure(&in_use) {
            None => {
                let Ok(sum) = in_use else {
                    return self.unavailable_volume(
                        UnavailableReason::AggregateMemberUnavailable,
                        "a face has no flux".to_owned(),
                        None,
                        orientation,
                        area_centroid,
                    );
                };
                let shell_volumes = sum
                    .shells
                    .iter()
                    .map(|volume| {
                        Some(measured(
                            Scalar::new(*volume),
                            Unit::CubicMillimeter,
                            Method::DivergenceQuadrature,
                            self.volume_errors(&sum.flux, *volume),
                            self.id,
                        ))
                    })
                    .collect();
                VolumeFacts {
                    volume: self.volume_quantity(&sum),
                    centroid: self.centroid_quantity(&sum),
                    area_centroid,
                    shell_volumes,
                    orientation,
                }
            }
            Some((reason, message, category)) => {
                self.unavailable_volume(reason, message, category, orientation, area_centroid)
            }
        }
    }

    fn unavailable_volume(
        &self,
        reason: UnavailableReason,
        message: String,
        category: Option<Category>,
        orientation: BodyOrientation,
        area_centroid: Option<Quantity<Point>>,
    ) -> VolumeFacts {
        let (volumes, centroids) = self.candidates(reason);
        let (volume, centroid) = match category {
            Some(category) => (
                Unavailable::aggregate(category, self.id, message.clone()),
                Unavailable::aggregate(category, self.id, message),
            ),
            None => (
                Unavailable::new(reason, Some(self.id), message.clone()),
                Unavailable::new(reason, Some(self.id), message),
            ),
        };
        VolumeFacts {
            volume: Quantity::Unavailable(volume.with_candidates(volumes)),
            centroid: Quantity::Unavailable(centroid.with_candidates(centroids)),
            area_centroid,
            shell_volumes: vec![None; self.shells.len()],
            orientation,
        }
    }

    /// The area-weighted centroid of a body with no closed shell.
    fn area_centroid(&self, sum: &Result<Sum, Missing>) -> Option<Quantity<Point>> {
        if self.shells.iter().all(|(closed, _)| *closed) && !self.shells.is_empty() {
            return None;
        }
        Some(match sum {
            Ok(sum) if self.area > 0.0 => {
                let moment = sum.flux.area_moment();
                let point = Point3::new(
                    self.origin.x + moment[0] / self.area,
                    self.origin.y + moment[1] / self.area,
                    self.origin.z + moment[2] / self.area,
                );
                let numeric = sum.flux.error[7]
                    .max(sum.flux.error[8])
                    .max(sum.flux.error[9]);
                measured(
                    Point::new(point),
                    Unit::Millimeter,
                    Method::DivergenceQuadrature,
                    Errors::new(numeric / self.area, 0.0),
                    self.id,
                )
            }
            Ok(_) => Quantity::missing(
                UnavailableReason::BoundaryUnboundedFace,
                Some(self.id),
                "the body has no area",
            ),
            Err(missing) => missing.quantity(),
        })
    }

    fn orientation(&self) -> BodyOrientation {
        let mut orientation = BodyOrientation {
            consistent: Some(true),
            inconsistent: Vec::new(),
            mixed: Vec::new(),
            unconfirmed: Vec::new(),
            conflict: Vec::new(),
            closure_flux: None,
        };
        let mut undetermined = false;
        for member in &self.members {
            let row = &member.result.row;
            match (
                member.result.verdict,
                row.orientation.consistent,
                row.orientation.loop_normal,
            ) {
                (Verdict::Conflict, ..) => orientation.conflict.push(member.id.to_owned()),
                (Verdict::Unconfirmed, ..) => {
                    orientation.unconfirmed.push(member.id.to_owned());
                    undetermined = true;
                }
                (_, Some(false), NormalReading::Mixed) => {
                    orientation.mixed.push(member.id.to_owned());
                }
                (_, Some(false), _) => orientation.inconsistent.push(member.id.to_owned()),
                (_, None, _) if row.measures.region != Some(RegionRule::WholeSurface) => {
                    undetermined = true;
                }
                _ => {}
            }
        }
        let disagrees = !(orientation.conflict.is_empty()
            && orientation.mixed.is_empty()
            && orientation.inconsistent.is_empty());
        orientation.consistent = if disagrees {
            Some(false)
        } else if undetermined {
            None
        } else {
            Some(true)
        };
        if !self.members.is_empty() {
            orientation.closure_flux = Some(ClosureFlux {
                face_sense: self.closure_quantity(BodyReading::FaceSense),
                loop_winding: self.closure_quantity(BodyReading::LoopWinding),
            });
        }
        orientation
    }
}

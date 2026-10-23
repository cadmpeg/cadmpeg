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

use crate::eigen::symmetric_eigen;
use crate::face::{FaceFlux, FaceResult, Verdict};
use crate::failure::Missing;
use crate::flux::Flux;
use crate::reason::{Category, UnavailableReason};
use crate::report::{BodyOrientation, ClosureFlux, NormalReading, RegionRule};
use crate::session::Density;
use crate::tolerance::PRINCIPAL_AXIS_RELATIVE_SEPARATION;
use crate::value::{
    measured, Candidate, Errors, Measured, Method, Point, Quantity, Reading, Scalar, Tensor,
    Unavailable, Unit,
};

/// Cubic millimetres in one cubic metre.
const CUBIC_MILLIMETRES_PER_CUBIC_METRE: f64 = 1.0e9;

/// The index pairs of a tensor's entries `xx yy zz xy yz zx`.
const TENSOR_ENTRIES: [(usize, usize); 6] = [(0, 0), (1, 1), (2, 2), (0, 1), (1, 2), (2, 0)];

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

/// Which region each face contributes to an area centroid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Regions {
    /// Every face's declared region.
    Declared,
    /// The region a witness or the empty-reading test establishes, else the
    /// declared region.
    Witnessed,
    /// As `Witnessed`, with every unconfirmed face in its complement
    /// region.
    Complement,
}

impl Regions {
    /// Whether a face with `verdict` contributes its complement region.
    const fn complement(self, verdict: Verdict) -> bool {
        match (self, verdict) {
            (Self::Declared, _) => false,
            (_, Verdict::Conflict | Verdict::Contradicted)
            | (Self::Complement, Verdict::Unconfirmed) => true,
            _ => false,
        }
    }
}

/// Whether two points agree within their summed error terms.
fn same_point(a: &Measured<Point>, b: &Measured<Point>) -> bool {
    let allowed = a.numeric_error().get()
        + b.numeric_error().get()
        + a.model_uncertainty().get()
        + b.model_uncertainty().get();
    let (first, second) = (a.value().value.get(), b.value().value.get());
    [first.x - second.x, first.y - second.y, first.z - second.z]
        .iter()
        .all(|difference| difference.abs() <= allowed)
}

/// Why a body's volume is unavailable.
#[derive(Debug, Clone)]
pub(super) struct Failure {
    pub(super) reason: UnavailableReason,
    pub(super) message: String,
    /// The cited member's category, for an aggregate reason.
    pub(super) category: Option<Category>,
    /// The faces the failure names.
    pub(super) related: Vec<String>,
    /// The region-order position of the shell the failure names.
    pub(super) shell: Option<usize>,
}

impl Failure {
    fn new(reason: UnavailableReason, message: String) -> Self {
        Self {
            reason,
            message,
            category: None,
            related: Vec::new(),
            shell: None,
        }
    }
}

/// One face of a body with its result.
#[derive(Clone, Copy)]
pub(super) struct Member<'a> {
    pub(super) id: &'a str,
    pub(super) shell: usize,
    pub(super) result: &'a FaceResult<'a>,
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
    /// The material density of mass and inertia.
    pub(super) density: Option<Density>,
}

/// The body's volume, centroid, area centroid, mass properties and
/// orientation facts.
pub(super) struct VolumeFacts {
    pub(super) volume: Quantity<Scalar>,
    pub(super) centroid: Quantity<Point>,
    pub(super) area_centroid: Option<Quantity<Point>>,
    pub(super) mass: MassFacts,
    pub(super) shell_volumes: Vec<Option<Quantity<Scalar>>>,
    pub(super) orientation: BodyOrientation,
}

/// Second moments of volume about the centroid, their principal moments and
/// axes, and, with a density, mass and inertia about the centroid.
pub(super) struct MassFacts {
    pub(super) second_moments: Quantity<Tensor>,
    pub(super) principal_moments: [Quantity<Scalar>; 3],
    pub(super) principal_axes: [Quantity<Point>; 3],
    pub(super) mass: Option<Quantity<Scalar>>,
    pub(super) inertia: Option<Quantity<Tensor>>,
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
                Verdict::Conflict | Verdict::Contradicted => complement(declared),
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

    /// The volume change when the boundary moves by the largest `τ_b`: the
    /// body area times that tolerance.
    fn boundary_term(&self) -> f64 {
        let tau = self
            .members
            .iter()
            .map(|member| member.result.tau_b)
            .fold(0.0, f64::max);
        self.area * tau
    }

    fn volume_errors(&self, flux: &Flux, value: f64) -> Errors {
        Errors::new(
            flux.volume_error(),
            self.boundary_term() + (self.placement)(3, value),
        )
    }

    /// The second moments of volume about the centroid, `M = S − m mᵀ / V`
    /// from the second moments `S` and first moments `m` about `P_ref`.
    /// The numeric error is the first-order propagation of the flux
    /// estimates. The model uncertainty is the change when the boundary
    /// moves by `τ_b`, the volume term times the squared body box diagonal,
    /// plus the placement term; the centroid's own move changes `M` only at
    /// second order.
    fn mass_facts(&self, sum: &Sum) -> MassFacts {
        let volume = sum.flux.volume();
        let moment = sum.flux.moment();
        let moment_errors = sum.flux.moment_errors();
        let volume_error = sum.flux.volume_error();
        let (second, second_errors) = sum.flux.second_moments();
        let central: [f64; 6] = std::array::from_fn(|k| {
            let (i, j) = TENSOR_ENTRIES[k];
            second[k] - moment[i] * moment[j] / volume
        });
        let numeric = (0..6)
            .map(|k| {
                let (i, j) = TENSOR_ENTRIES[k];
                second_errors[k]
                    + (moment[i].abs() * moment_errors[j] + moment[j].abs() * moment_errors[i])
                        / volume.abs()
                    + (moment[i] * moment[j]).abs() * volume_error / (volume * volume)
            })
            .fold(0.0, f64::max);
        let largest = central
            .iter()
            .fold(0.0_f64, |most, entry| most.max(entry.abs()));
        let model =
            self.boundary_term() * self.diagonal * self.diagonal + (self.placement)(5, largest);
        let errors = Errors::new(numeric, model);
        let second_moments = measured(
            Tensor::new(central),
            Unit::MillimeterToTheFifth,
            Method::DivergenceQuadrature,
            errors,
            self.id,
        );
        let (principal_moments, principal_axes) = match &second_moments {
            Quantity::Measured(tensor) => self.principal(&tensor.value().rows(), errors),
            Quantity::Unavailable(unavailable) => (
                std::array::from_fn(|_| Quantity::Unavailable(unavailable.recast())),
                std::array::from_fn(|_| Quantity::Unavailable(unavailable.recast())),
            ),
        };
        let (mass, inertia) = match self.density {
            Some(density) => {
                let per_cubic_millimetre = density.get() / CUBIC_MILLIMETRES_PER_CUBIC_METRE;
                let volume_errors = self.volume_errors(&sum.flux, volume);
                let mass = measured(
                    Scalar::new(per_cubic_millimetre * volume),
                    Unit::Kilogram,
                    Method::DivergenceQuadrature,
                    Errors::new(
                        per_cubic_millimetre * volume_errors.numeric,
                        per_cubic_millimetre * volume_errors.model,
                    ),
                    self.id,
                );
                let [xx, yy, zz, xy, yz, zx] = central;
                let inertia = measured(
                    Tensor::new(
                        [yy + zz, zz + xx, xx + yy, -xy, -yz, -zx]
                            .map(|entry| per_cubic_millimetre * entry),
                    ),
                    Unit::KilogramSquareMillimeter,
                    Method::DivergenceQuadrature,
                    Errors::new(
                        2.0 * per_cubic_millimetre * numeric,
                        2.0 * per_cubic_millimetre * model,
                    ),
                    self.id,
                );
                (Some(mass), Some(inertia))
            }
            None => (None, None),
        };
        MassFacts {
            second_moments,
            principal_moments,
            principal_axes,
            mass,
            inertia,
        }
    }

    /// The principal moments of `matrix` and its principal axes. The axis
    /// of a moment is `mass.principal-axes-degenerate` when another moment
    /// agrees with it within `PRINCIPAL_AXIS_RELATIVE_SEPARATION` of the
    /// largest plus their error terms. A matrix perturbation of entries at
    /// most `e` moves each eigenvalue by at most `3e` and the eigenvector of
    /// an isolated eigenvalue by at most that over its gap to the others.
    fn principal(
        &self,
        matrix: &[[f64; 3]; 3],
        errors: Errors,
    ) -> ([Quantity<Scalar>; 3], [Quantity<Point>; 3]) {
        let eigen = symmetric_eigen(*matrix);
        let moment_errors = Errors::new(3.0 * errors.numeric + eigen.residual, 3.0 * errors.model);
        let moments = eigen.values.map(|value| {
            measured(
                Scalar::new(value),
                Unit::MillimeterToTheFifth,
                Method::DivergenceQuadrature,
                moment_errors,
                self.id,
            )
        });
        let largest = eigen
            .values
            .iter()
            .fold(0.0_f64, |most, value| most.max(value.abs()));
        let separation = PRINCIPAL_AXIS_RELATIVE_SEPARATION * largest
            + 2.0 * (moment_errors.numeric + moment_errors.model);
        let axes = std::array::from_fn(|k| {
            let gap = (0..3)
                .filter(|j| *j != k)
                .map(|j| (eigen.values[k] - eigen.values[j]).abs())
                .fold(f64::INFINITY, f64::min);
            if gap > separation {
                let [x, y, z] = eigen.vectors[k];
                measured(
                    Point::new(Point3::new(x, y, z)),
                    Unit::One,
                    Method::DivergenceQuadrature,
                    Errors::new(moment_errors.numeric / gap, moment_errors.model / gap),
                    self.id,
                )
            } else {
                Quantity::missing(
                    UnavailableReason::MassPrincipalAxesDegenerate,
                    Some(self.id),
                    format!(
                        "principal moment {} is {gap} mm⁵ from another, within their \
                         separation {separation} mm⁵",
                        k + 1
                    ),
                )
            }
        });
        (moments, axes)
    }

    /// Mass properties unavailable as the volume is.
    fn unavailable_mass(
        &self,
        reason: UnavailableReason,
        message: &str,
        category: Option<Category>,
    ) -> MassFacts {
        let missing = || match category {
            Some(category) => Unavailable::<()>::aggregate(category, self.id, message),
            None => Unavailable::new(reason, Some(self.id), message),
        };
        MassFacts {
            second_moments: Quantity::Unavailable(missing().recast()),
            principal_moments: std::array::from_fn(|_| Quantity::Unavailable(missing().recast())),
            principal_axes: std::array::from_fn(|_| Quantity::Unavailable(missing().recast())),
            mass: self
                .density
                .map(|_| Quantity::Unavailable(missing().recast())),
            inertia: self
                .density
                .map(|_| Quantity::Unavailable(missing().recast())),
        }
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
        let moment_error = sum.flux.moment_error();
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
        let numeric = sum.flux.closure_error();
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

    /// The first failing precondition of a measured volume.
    fn failure(&self, sum: &Result<Sum, Missing>) -> Option<Failure> {
        if self.shells.iter().any(|(closed, _)| !closed) {
            return Some(Failure::new(
                UnavailableReason::VolumeShellOpen,
                "a shell of the body is open".to_owned(),
            ));
        }
        let sum = match sum {
            Ok(sum) => sum,
            Err(missing) => {
                let category = missing.reason.category().unwrap_or(Category::Input);
                return Some(Failure {
                    category: Some(category),
                    ..Failure::new(
                        UnavailableReason::AggregateMemberUnavailable,
                        format!(
                            "member {} has no flux: {}",
                            missing.entity.as_deref().unwrap_or("?"),
                            missing.reason.code()
                        ),
                    )
                });
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
                (Verdict::Conflict | Verdict::Contradicted, ..) | (_, _, Some(false)) => {
                    conflict.push(member.id);
                }
                (Verdict::Unconfirmed, ..) => unverified.push(member.id),
                (_, Some(RegionRule::WholeSurface), _) | (_, _, Some(true)) => {}
                (_, _, None) => unverified.push(member.id),
            }
        }
        if !conflict.is_empty() {
            return Some(Failure {
                related: conflict.iter().map(|face| (*face).to_owned()).collect(),
                ..Failure::new(
                    UnavailableReason::VolumeOrientationConflict,
                    format!(
                        "{} face(s) disagree with their readings: {}",
                        conflict.len(),
                        conflict.join(", ")
                    ),
                )
            });
        }
        if !unverified.is_empty() {
            return Some(Failure::new(
                UnavailableReason::VolumeOrientationUnverified,
                format!(
                    "{} face(s) rest on an unconfirmed orientation: {}",
                    unverified.len(),
                    unverified.join(", ")
                ),
            ));
        }
        let (magnitude, tolerance, _) = self.closure(sum);
        if magnitude > tolerance {
            return Some(Failure::new(
                UnavailableReason::VolumeClosureFlux,
                format!("the closure flux {magnitude} mm² exceeds its tolerance {tolerance} mm²"),
            ));
        }
        for (position, ((_, outer), volume)) in self.shells.iter().zip(&sum.shells).enumerate() {
            let wrong = if *outer {
                *volume <= 0.0
            } else {
                *volume >= 0.0
            };
            if wrong {
                return Some(Failure {
                    shell: Some(position),
                    ..Failure::new(
                        UnavailableReason::VolumeOrientationConflict,
                        format!("shell {position} has a volume of the wrong sign for its role"),
                    )
                });
            }
        }
        None
    }

    /// The failure of a body whose shells are closed and whose volume
    /// readings conflict: an orientation conflict or a closure flux above
    /// its tolerance. `None` when the volume is measured or fails for
    /// another reason.
    pub(super) fn conflict(&self) -> Option<Failure> {
        self.failure(&self.sum(BodyReading::FaceSense))
            .filter(|failure| {
                matches!(
                    failure.reason,
                    UnavailableReason::VolumeOrientationConflict
                        | UnavailableReason::VolumeClosureFlux
                )
            })
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
        let area_centroid = self.area_centroid();
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
                    mass: self.mass_facts(&sum),
                    shell_volumes,
                    orientation,
                }
            }
            Some(failure) => self.unavailable_volume(
                failure.reason,
                failure.message,
                failure.category,
                orientation,
                area_centroid,
            ),
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
        let mass = self.unavailable_mass(reason, &message, category);
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
            mass,
            shell_volumes: vec![None; self.shells.len()],
            orientation,
        }
    }

    /// The area moments, area and area error of the body's faces under
    /// `regions`.
    fn area_sum(&self, regions: Regions) -> Result<(Flux, f64, f64), Missing> {
        let mut flux_sum = Flux::ZERO;
        let (mut area, mut area_error) = (0.0, 0.0);
        for member in &self.members {
            let result = member.result;
            let complement = regions.complement(result.verdict);
            let missing = || Missing {
                reason: UnavailableReason::AggregateMemberUnavailable,
                entity: Some(member.id.to_owned()),
                message: format!("member {} has no flux", member.id),
            };
            let flux = match &result.flux {
                Some(Ok(flux)) if complement => flux.complement.ok_or_else(missing)?,
                Some(Ok(flux)) => flux.declared,
                Some(Err(missing)) => return Err(missing.clone()),
                None => return Err(missing()),
            };
            match result.area_reading(complement) {
                Quantity::Measured(measured) => {
                    area += measured.value().value.get();
                    area_error += measured.numeric_error().get();
                }
                Quantity::Unavailable(unavailable) => {
                    return Err(Missing {
                        reason: unavailable.reason(),
                        entity: Some(unavailable.entity().unwrap_or(member.id).to_owned()),
                        message: unavailable.message().to_owned(),
                    })
                }
            }
            flux_sum = flux_sum.add(flux, 1.0);
        }
        Ok((flux_sum, area, area_error))
    }

    /// The area centroid `P_ref + M_A / A` of the faces under `regions`.
    /// Its model uncertainty is the first-order move of the centroid when
    /// the boundary moves by `τ_b`: the summed boundary length times `τ_b`
    /// plus the placement term of the area, times the body box diagonal,
    /// over the area.
    fn area_centroid_under(&self, regions: Regions) -> Quantity<Point> {
        let (flux, area, area_error) = match self.area_sum(regions) {
            Ok(found) => found,
            Err(missing) => return missing.quantity(),
        };
        if area <= 0.0 {
            return Quantity::missing(
                UnavailableReason::BoundaryUnboundedFace,
                Some(self.id),
                "the body has no area",
            );
        }
        let moment = flux.area_moment();
        let point = Point3::new(
            self.origin.x + moment[0] / area,
            self.origin.y + moment[1] / area,
            self.origin.z + moment[2] / area,
        );
        let moment_error = flux.area_moment_error();
        let lever = moment.iter().fold(0.0_f64, |most, m| most.max(m.abs()));
        let boundary: f64 = self
            .members
            .iter()
            .map(|member| member.result.boundary_length * member.result.tau_b)
            .sum();
        measured(
            Point::new(point),
            Unit::Millimeter,
            Method::DivergenceQuadrature,
            Errors::new(
                moment_error / area + lever * area_error / (area * area),
                (boundary + (self.placement)(2, area)) * self.diagonal / area,
            ),
            self.id,
        )
    }

    /// The area-weighted centroid of a body with no closed shell, over the
    /// regions the faces' witnesses establish. It rests on the declared
    /// reading of every unconfirmed face, with the complement candidate; it
    /// is unavailable with the `face_sense` and `loop_winding` candidates
    /// when a witness contradicts a face.
    fn area_centroid(&self) -> Option<Quantity<Point>> {
        if self.shells.iter().all(|(closed, _)| *closed) && !self.shells.is_empty() {
            return None;
        }
        let verdicts = || {
            self.members
                .iter()
                .map(|member| (member.id, member.result.verdict))
        };
        if let Some((face, _)) = verdicts().find(|(_, verdict)| *verdict == Verdict::Contradicted) {
            let candidates = [
                (Regions::Declared, Reading::FaceSense),
                (Regions::Witnessed, Reading::LoopWinding),
            ]
            .into_iter()
            .filter_map(
                |(regions, reading)| match self.area_centroid_under(regions) {
                    Quantity::Measured(measured) => Some(measured.into_candidate(reading)),
                    Quantity::Unavailable(_) => None,
                },
            )
            .collect();
            return Some(Quantity::Unavailable(
                Unavailable::new(
                    UnavailableReason::RegionOrientationConflict,
                    Some(face),
                    "a witness contradicts the declared orientation of a face the area centroid rests on",
                )
                .with_candidates(candidates),
            ));
        }
        let main = self.area_centroid_under(Regions::Witnessed);
        if !verdicts().any(|(_, verdict)| verdict == Verdict::Unconfirmed) {
            return Some(main);
        }
        let Quantity::Measured(declared) = main else {
            return Some(main);
        };
        Some(Quantity::Measured(
            match self.area_centroid_under(Regions::Complement) {
                Quantity::Measured(complement) if same_point(&declared, &complement) => declared,
                Quantity::Measured(complement) => declared.under_reading(
                    Reading::Declared,
                    vec![complement.into_candidate(Reading::Complement)],
                ),
                Quantity::Unavailable(_) => declared.under_reading(Reading::Declared, Vec::new()),
            },
        ))
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
                (Verdict::Conflict | Verdict::Contradicted, ..) => {
                    orientation.conflict.push(member.id.to_owned());
                }
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

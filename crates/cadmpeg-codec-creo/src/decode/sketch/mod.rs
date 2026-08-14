// SPDX-License-Identifier: Apache-2.0
//! Section geometry conversion and sketch-table coordinate, radius, and trim solvers.

mod coordinates;
mod equations_coordinate;
mod equations_scalar;
mod geometry;
mod intersect;
mod radii;
mod skamp;

#[allow(clippy::wildcard_imports)]
pub(crate) use coordinates::*;
#[allow(clippy::wildcard_imports)]
pub(crate) use equations_coordinate::*;
#[allow(clippy::wildcard_imports)]
pub(crate) use equations_scalar::*;
#[allow(clippy::wildcard_imports)]
pub(crate) use geometry::*;
#[allow(clippy::wildcard_imports)]
pub(crate) use intersect::*;
#[allow(clippy::wildcard_imports)]
pub(crate) use radii::*;
#[allow(clippy::wildcard_imports)]
pub(crate) use skamp::*;

#[cfg(test)]
mod tests;

// SPDX-License-Identifier: Apache-2.0
//! Section geometry conversion and sketch-table coordinate, radius, and trim solvers.

pub(super) mod axis;
pub(super) mod coordinates;
pub(super) mod equations_coordinate;
pub(super) mod equations_scalar;
pub(super) mod geometry;
pub(super) mod intersect;
pub(super) mod radii;
pub(super) mod skamp;

#[cfg(test)]
mod tests;

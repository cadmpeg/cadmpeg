// SPDX-License-Identifier: Apache-2.0
//! Adaptive Gauss–Kronrod quadrature and extremum search along a parameter
//! interval.

mod extremum;
mod gauss_kronrod;

pub(crate) use extremum::extrema;
pub(crate) use gauss_kronrod::{integrate, nodes as gauss_kronrod_nodes, Target};

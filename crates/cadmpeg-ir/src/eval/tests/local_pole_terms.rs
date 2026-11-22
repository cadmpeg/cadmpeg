// SPDX-License-Identifier: Apache-2.0
//! The local pole traversal against the row-by-row traversal it replaces.

use crate::eval::LocalPoleTerms;
use crate::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
use crate::math::Point3;
use crate::scalar::NonZeroReal;

fn rational_grid() -> NurbsSurface {
    let points = (0..3_u32)
        .map(|i| {
            (0..4_u32)
                .map(|j| Point3::new(f64::from(i), f64::from(j), f64::from(i * j)))
                .collect()
        })
        .collect();
    let weights = (0..3_u32)
        .map(|i| {
            (0..4_u32)
                .map(|j| 1.0 + f64::from(i + 2 * j) / 8.0)
                .collect()
        })
        .collect();
    NurbsSurface::from_lanes(
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 0.5, 1.0, 1.0], false),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 2.0, 3.0, 3.0], false),
        NurbsSurfaceLanes::new(points, Some(weights)),
        false,
    )
    .expect("a consistent grid")
}

type Term = Option<([f64; 2], f64, crate::features::geometry::FinitePoint3)>;

/// A draw's bounds before it, and the term it drew.
type Draw = ((usize, Option<usize>), Option<Term>);

/// Each draw, until the traversal ends.
fn draws(mut terms: impl Iterator<Item = Term>) -> Vec<Draw> {
    let mut draws = Vec::new();
    loop {
        let bounds = terms.size_hint();
        let term = terms.next();
        let end = term.is_none();
        draws.push((bounds, term));
        if end {
            return draws;
        }
    }
}

#[test]
fn local_pole_terms_draw_as_the_row_by_row_traversal() {
    let surface = &rational_grid();
    let values = [0.25, -0.5, 2.0];
    // The `u` window starting at row 2 reaches past the last row, whose
    // poles are missing.
    for first in [[0, 0], [1, 1], [2, 0]] {
        for rows in 0..=3 {
            for columns in 0..=3 {
                let (u_values, v_values) = (&values[..rows], &values[..columns]);
                let row_by_row = u_values
                    .iter()
                    .copied()
                    .enumerate()
                    .flat_map(|(i, u_value)| {
                        v_values
                            .iter()
                            .copied()
                            .enumerate()
                            .map(move |(j, v_value)| {
                                let (pole_u, pole_v) = (first[0] + i, first[1] + j);
                                Some((
                                    [u_value, v_value],
                                    surface.weight(pole_u, pole_v).map_or(1.0, NonZeroReal::get),
                                    surface.pole(pole_u, pole_v)?,
                                ))
                            })
                    });
                let local = LocalPoleTerms {
                    surface,
                    first,
                    values: [u_values, v_values],
                    at: [0, 0],
                };
                assert_eq!(
                    draws(local),
                    draws(row_by_row),
                    "first {first:?}, {rows} by {columns}"
                );
            }
        }
    }
}

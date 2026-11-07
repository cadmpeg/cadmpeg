// SPDX-License-Identifier: Apache-2.0
//! Emit placement matrices as regular entities so references can be ordered.

use super::{number, Entity, EntityStatus, IgesLossCode, LossNote, Placement};

pub(super) fn materialize(entities: &mut Vec<Entity>) -> Vec<LossNote> {
    let mut losses = Vec::new();
    for index in 0..entities.len() {
        let Some(placement) = entities[index].transform.take() else {
            continue;
        };
        debug_assert!(
            entities[index].transform_link.is_none(),
            "IGES entity has either a placement or a transformation link"
        );
        let Some(form) = matrix_form(placement) else {
            if entities[index].source_targets.is_empty() {
                losses.push(IgesLossCode::WriterBodyTransformNotApplied.note(format!(
                    "IGES type {} entity {index} placement is not an orthonormal Type 124 matrix and is not applied",
                    entities[index].type_code
                )));
            }
            for id in &entities[index].source_targets {
                losses.push(IgesLossCode::WriterBodyTransformNotApplied.note(format!(
                    "IGES record {id} placement is not an orthonormal Type 124 matrix and is not applied"
                )));
            }
            continue;
        };
        let body = placement
            .rows
            .iter()
            .flatten()
            .map(|value| number(*value))
            .collect::<Vec<_>>()
            .join(",");
        let transform = entities.len();
        entities[index].transform_link = Some(transform);
        entities.push(Entity {
            structure: None,
            property_links: Vec::new(),
            association_links: Vec::new(),
            line_font: None,
            source_targets: Vec::new(),
            type_code: 124,
            form,
            label: String::new(),
            status: EntityStatus::Independent,
            parameter_body: format!("{body};").into_bytes(),
            transform: None,
            transform_link: None,
            subscript: 0,
        });
    }
    losses
}

// Apply the same declared 17-digit significance and outward rounding used to
// read generated Type 124 coefficients. A frame whose intervals do not contain
// the required invariants cannot be written as a Type 124 matrix.
#[derive(Clone, Copy)]
struct Interval {
    lower: f64,
    upper: f64,
}

impl Interval {
    fn exact(value: f64) -> Self {
        Self {
            lower: value,
            upper: value,
        }
    }
    fn outward(lower: f64, upper: f64) -> Self {
        Self {
            lower: lower.next_down(),
            upper: upper.next_up(),
        }
    }
    fn coefficient(value: f64) -> Self {
        if value == 0.0 {
            return Self::exact(0.0);
        }
        let uncertainty = 0.5 * 10.0_f64.powf(value.abs().log10().floor() - 16.0);
        if uncertainty == 0.0 {
            Self::exact(value)
        } else {
            Self::outward(value - uncertainty, value + uncertainty)
        }
    }
    fn add(self, other: Self) -> Self {
        Self::outward(self.lower + other.lower, self.upper + other.upper)
    }
    fn subtract(self, other: Self) -> Self {
        Self::outward(self.lower - other.upper, self.upper - other.lower)
    }
    fn multiply(self, other: Self) -> Self {
        let values = [
            self.lower * other.lower,
            self.lower * other.upper,
            self.upper * other.lower,
            self.upper * other.upper,
        ];
        Self::outward(
            values.into_iter().fold(f64::INFINITY, f64::min),
            values.into_iter().fold(f64::NEG_INFINITY, f64::max),
        )
    }
    fn contains(self, value: f64) -> bool {
        self.lower <= value && value <= self.upper
    }
}

pub(super) fn matrix_form(placement: Placement) -> Option<i64> {
    let values: [[Interval; 3]; 3] = std::array::from_fn(|row| {
        std::array::from_fn(|column| Interval::coefficient(placement.rows[row][column].get()))
    });
    let dot = |left: usize, right: usize| {
        (0..3).fold(Interval::exact(0.0), |sum, row| {
            sum.add(values[row][left].multiply(values[row][right]))
        })
    };
    if (0..3).any(|column| !dot(column, column).contains(1.0))
        || [(0, 1), (0, 2), (1, 2)]
            .into_iter()
            .any(|(a, b)| !dot(a, b).contains(0.0))
    {
        return None;
    }
    let v = |r: usize, c: usize| values[r][c];
    let determinant = v(0, 0)
        .multiply(
            v(1, 1)
                .multiply(v(2, 2))
                .subtract(v(1, 2).multiply(v(2, 1))),
        )
        .subtract(
            v(0, 1).multiply(
                v(1, 0)
                    .multiply(v(2, 2))
                    .subtract(v(1, 2).multiply(v(2, 0))),
            ),
        )
        .add(
            v(0, 2).multiply(
                v(1, 0)
                    .multiply(v(2, 1))
                    .subtract(v(1, 1).multiply(v(2, 0))),
            ),
        );
    if determinant.contains(1.0) {
        Some(0)
    } else if determinant.contains(-1.0) {
        Some(1)
    } else {
        None
    }
}

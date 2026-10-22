// SPDX-License-Identifier: Apache-2.0
//! Sums and box unions of member values for body and document rows.
//!
//! A member value measured under the `declared` reading of an unconfirmed
//! face makes the aggregate rest on that reading: the aggregate carries
//! `reading: declared`, and a `complement` candidate that takes every such
//! member under its `complement` candidate. When one such member has no
//! complement candidate, the aggregate carries the reading and no
//! candidate.
//!
//! The union of member boxes is tight only when every member box is tight.
//! A conservative union carries a lower bound of its tight box: the union of
//! the tight member boxes and of the boundary boxes of the conservative
//! members. Every point of such a member lies on the entity, so the lower
//! bound lies inside the entity's tight box.

use crate::face::box_value;
use crate::geom::Aabb;
use crate::reason::UnavailableReason;
use crate::value::{
    measured, BoxKind, BoxValue, Errors, Measured, Method, Quantity, Reading, Scalar, Unavailable,
    Unit,
};

/// The corners of a box value as an accumulated box.
pub(crate) fn corners(value: &BoxValue) -> Aabb {
    let mut aabb = Aabb::of_point(value.min.get());
    aabb.include(value.max.get());
    aabb
}

/// The lower bound of the tight box a box value states: the box itself
/// when it is tight, else its boundary box.
pub(crate) fn lower_bound(value: &BoxValue) -> Option<Aabb> {
    match (value.kind, &value.boundary) {
        (BoxKind::Tight, _) => Some(corners(value)),
        (BoxKind::Conservative, Some(boundary)) => {
            let mut aabb = Aabb::of_point(boundary.min.get());
            aabb.include(boundary.max.get());
            Some(aabb)
        }
        (BoxKind::Conservative, None) => None,
    }
}

/// Whether an aggregate rests on the declared reading of a member, and
/// whether every such member has a complement candidate.
#[derive(Debug, Clone, Copy)]
struct Resting {
    declared: bool,
    complete: bool,
}

impl Resting {
    const NONE: Self = Self {
        declared: false,
        complete: true,
    };

    /// Note one measured member; return its complement candidate's value
    /// and errors, when it has one.
    fn note<'a, T>(&mut self, measured: &'a Measured<T>) -> Option<(&'a T, Errors)> {
        let complement = measured
            .candidates()
            .iter()
            .find(|candidate| candidate.reading() == Reading::Complement)
            .map(|candidate| {
                (
                    candidate.value(),
                    Errors::new(
                        candidate.numeric_error().get(),
                        candidate.model_uncertainty().get(),
                    ),
                )
            });
        if measured.reading() == Some(Reading::Declared) {
            self.declared = true;
            self.complete &= complement.is_some();
        }
        complement
    }

    /// `main` under the declared reading with `complement` as its
    /// candidate, when the aggregate rests on a declared reading.
    fn apply<T>(self, main: Quantity<T>, complement: Quantity<T>) -> Quantity<T> {
        let Quantity::Measured(main) = main else {
            return main;
        };
        if !self.declared {
            return Quantity::Measured(main);
        }
        let candidates = match complement {
            Quantity::Measured(complement) if self.complete => {
                vec![complement.into_candidate(Reading::Complement)]
            }
            _ => Vec::new(),
        };
        Quantity::Measured(main.under_reading(Reading::Declared, candidates))
    }
}

fn errors_of<T>(measured: &Measured<T>) -> Errors {
    Errors::new(
        measured.numeric_error().get(),
        measured.model_uncertainty().get(),
    )
}

/// The sum of member values; errors add.
pub(crate) struct ReadingSum {
    values: [f64; 2],
    errors: [Errors; 2],
    resting: Resting,
    missing: Option<Unavailable<Scalar>>,
}

impl ReadingSum {
    pub(crate) const fn new() -> Self {
        Self {
            values: [0.0; 2],
            errors: [Errors::ZERO; 2],
            resting: Resting::NONE,
            missing: None,
        }
    }

    /// Add one member's quantity; `member` and `what` name it in the
    /// aggregate reason of an unavailable member.
    pub(crate) fn add(&mut self, member: &str, what: &str, quantity: &Quantity<Scalar>) {
        match quantity {
            Quantity::Measured(measured) => {
                let main = (measured.value().value.get(), errors_of(measured));
                let complement = self
                    .resting
                    .note(measured)
                    .map_or(main, |(value, errors)| (value.value.get(), errors));
                for (slot, (value, errors)) in [main, complement].into_iter().enumerate() {
                    self.values[slot] += value;
                    self.errors[slot] = self.errors[slot].add(errors);
                }
            }
            Quantity::Unavailable(unavailable) => {
                if self.missing.is_none() {
                    self.missing = Some(Unavailable::aggregate(
                        unavailable.category(),
                        member,
                        format!(
                            "member {member} {what} is unavailable: {}",
                            unavailable.reason().code()
                        ),
                    ));
                }
            }
        }
    }

    /// The sum, or the first unavailable member.
    pub(crate) fn finish(self, unit: Unit, entity: &str) -> Quantity<Scalar> {
        if let Some(missing) = self.missing {
            return Quantity::Unavailable(missing);
        }
        let quantity = |slot: usize| {
            measured(
                Scalar::new(self.values[slot]),
                unit,
                Method::Sum,
                self.errors[slot],
                entity,
            )
        };
        self.resting.apply(quantity(0), quantity(1))
    }
}

/// One reading of a box union.
#[derive(Debug, Clone, Copy)]
struct Union {
    upper: Option<Aabb>,
    lower: Option<Aabb>,
    kind: BoxKind,
    errors: Errors,
}

fn join(aabb: Option<Aabb>, other: Aabb) -> Aabb {
    aabb.map_or(other, |aabb| aabb.union(other))
}

impl Union {
    const EMPTY: Self = Self {
        upper: None,
        lower: None,
        kind: BoxKind::Tight,
        errors: Errors::ZERO,
    };

    fn include(&mut self, aabb: Aabb, kind: BoxKind, lower: Option<Aabb>, errors: Errors) {
        self.upper = Some(join(self.upper, aabb));
        if let Some(lower) = lower {
            self.lower = Some(join(self.lower, lower));
        }
        if kind == BoxKind::Conservative {
            self.kind = BoxKind::Conservative;
        }
        self.errors = Errors::new(
            self.errors.numeric.max(errors.numeric),
            self.errors.model.max(errors.model),
        );
    }

    fn include_value(&mut self, value: &BoxValue, errors: Errors) {
        self.include(corners(value), value.kind, lower_bound(value), errors);
    }

    fn quantity(self, entity: &str) -> Option<Quantity<BoxValue>> {
        let upper = self.upper?;
        let lower = match self.kind {
            BoxKind::Tight => None,
            BoxKind::Conservative => self.lower,
        };
        Some(measured(
            box_value(upper, self.kind, lower),
            Unit::Millimeter,
            Method::Sum,
            self.errors,
            entity,
        ))
    }
}

/// The union of member boxes, with the first unavailable member.
pub(crate) struct BoxUnion {
    unions: [Union; 2],
    resting: Resting,
    missing: Option<Unavailable<BoxValue>>,
}

impl BoxUnion {
    pub(crate) const fn new() -> Self {
        Self {
            unions: [Union::EMPTY; 2],
            resting: Resting::NONE,
            missing: None,
        }
    }

    /// Add a box with its kind, its lower bound and its error terms.
    pub(crate) fn include(
        &mut self,
        aabb: Aabb,
        kind: BoxKind,
        lower: Option<Aabb>,
        errors: Errors,
    ) {
        for union in &mut self.unions {
            union.include(aabb, kind, lower, errors);
        }
    }

    /// Add one member's box quantity; `member` names it in the aggregate
    /// reason of an unavailable member.
    pub(crate) fn add(&mut self, member: &str, quantity: &Quantity<BoxValue>) {
        match quantity {
            Quantity::Measured(measured) => {
                let main = (measured.value(), errors_of(measured));
                let complement = self.resting.note(measured).unwrap_or(main);
                let [declared, other] = &mut self.unions;
                declared.include_value(main.0, main.1);
                other.include_value(complement.0, complement.1);
            }
            Quantity::Unavailable(unavailable) => self.miss(member, unavailable),
        }
    }

    /// Record an unavailable member.
    pub(crate) fn miss<T>(&mut self, member: &str, unavailable: &Unavailable<T>) {
        if self.missing.is_none() {
            self.missing = Some(Unavailable::aggregate(
                unavailable.category(),
                member,
                format!(
                    "member {member} box is unavailable: {}",
                    unavailable.reason().code()
                ),
            ));
        }
    }

    /// The union, the first unavailable member, or `boundary.unbounded-face`
    /// with `message` when no member has a box.
    pub(crate) fn finish(self, entity: Option<&str>, message: &str) -> Quantity<BoxValue> {
        if let Some(missing) = self.missing {
            return Quantity::Unavailable(missing);
        }
        let named = entity.unwrap_or("document");
        let [declared, complement] = self.unions.map(|union| union.quantity(named));
        let (Some(declared), Some(complement)) = (declared, complement) else {
            return Quantity::missing(UnavailableReason::BoundaryUnboundedFace, entity, message);
        };
        self.resting.apply(declared, complement)
    }
}

#[cfg(test)]
mod tests {
    use cadmpeg_ir::math::Point3;

    use super::{lower_bound, BoxUnion, ReadingSum};
    use crate::geom::Aabb;
    use crate::value::{measured, BoxKind, Errors, Method, Quantity, Reading, Scalar, Unit};

    fn scalar(value: f64) -> Quantity<Scalar> {
        measured(
            Scalar::new(value),
            Unit::SquareMillimeter,
            Method::BoundaryQuadrature,
            Errors::new(0.0, 1.0),
            "test:model:face#a",
        )
    }

    /// `declared` under the declared reading, with `complement` as its
    /// complement candidate when given.
    fn declared(declared: f64, complement: Option<f64>) -> Quantity<Scalar> {
        let Quantity::Measured(main) = scalar(declared) else {
            panic!("measured");
        };
        let candidates = complement
            .and_then(|value| scalar(value).measured().cloned())
            .map(|other| other.into_candidate(Reading::Complement))
            .into_iter()
            .collect();
        Quantity::Measured(main.under_reading(Reading::Declared, candidates))
    }

    #[test]
    fn a_sum_over_a_declared_member_rests_on_its_reading() {
        let mut sum = ReadingSum::new();
        sum.add("test:model:face#a", "area", &scalar(10.0));
        sum.add("test:model:face#b", "area", &declared(5.0, Some(7.0)));
        let quantity = sum.finish(Unit::SquareMillimeter, "test:model:body#a");
        let total = quantity.measured().expect("measured");
        assert_eq!(total.value().value.get(), 15.0);
        assert_eq!(total.model_uncertainty().get(), 2.0);
        assert_eq!(total.reading(), Some(Reading::Declared));
        let [candidate] = total.candidates() else {
            panic!("one candidate");
        };
        assert_eq!(candidate.reading(), Reading::Complement);
        assert_eq!(candidate.value().value.get(), 17.0);

        let mut sum = ReadingSum::new();
        sum.add("test:model:face#a", "area", &declared(5.0, None));
        sum.add("test:model:face#b", "area", &declared(5.0, Some(7.0)));
        let quantity = sum.finish(Unit::SquareMillimeter, "test:model:body#a");
        let total = quantity.measured().expect("measured");
        assert_eq!(total.reading(), Some(Reading::Declared));
        assert!(total.candidates().is_empty());

        let mut sum = ReadingSum::new();
        sum.add("test:model:face#a", "area", &scalar(10.0));
        let quantity = sum.finish(Unit::SquareMillimeter, "test:model:body#a");
        assert_eq!(quantity.measured().expect("measured").reading(), None);
    }

    fn aabb(min: [f64; 3], max: [f64; 3]) -> Aabb {
        let mut aabb = Aabb::of_point(Point3::new(min[0], min[1], min[2]));
        aabb.include(Point3::new(max[0], max[1], max[2]));
        aabb
    }

    #[test]
    fn a_conservative_union_bounds_its_tight_box_from_below() {
        let mut union = BoxUnion::new();
        union.include(
            aabb([0.0; 3], [1.0; 3]),
            BoxKind::Tight,
            Some(aabb([0.0; 3], [1.0; 3])),
            Errors::ZERO,
        );
        union.include(
            aabb([-1.0; 3], [3.0; 3]),
            BoxKind::Conservative,
            Some(aabb([1.0; 3], [2.0; 3])),
            Errors::ZERO,
        );
        union.include(
            aabb([5.0; 3], [6.0; 3]),
            BoxKind::Conservative,
            None,
            Errors::ZERO,
        );
        let quantity = union.finish(Some("test:model:body#a"), "no box");
        let value = quantity.measured().expect("measured").value();
        assert_eq!(value.kind, BoxKind::Conservative);
        assert_eq!(value.min.get(), Point3::new(-1.0, -1.0, -1.0));
        assert_eq!(value.max.get(), Point3::new(6.0, 6.0, 6.0));
        assert_eq!(lower_bound(value), Some(aabb([0.0; 3], [2.0; 3])));
    }

    #[test]
    fn a_tight_union_is_its_own_lower_bound() {
        let mut union = BoxUnion::new();
        union.include(aabb([0.0; 3], [1.0; 3]), BoxKind::Tight, None, Errors::ZERO);
        union.include(aabb([2.0; 3], [3.0; 3]), BoxKind::Tight, None, Errors::ZERO);
        let quantity = union.finish(Some("test:model:body#a"), "no box");
        let value = quantity.measured().expect("measured").value();
        assert_eq!(value.kind, BoxKind::Tight);
        assert_eq!(value.boundary, None);
        assert_eq!(lower_bound(value), Some(aabb([0.0; 3], [3.0; 3])));
    }
}

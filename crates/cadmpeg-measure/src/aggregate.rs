// SPDX-License-Identifier: Apache-2.0
//! Unions of member boxes for body and document rows.
//!
//! The union of member boxes is tight only when every member box is tight.
//! A conservative union carries a lower bound of its tight box: the union of
//! the tight member boxes and of the boundary boxes of the conservative
//! members. Every point of such a member lies on the entity, so the lower
//! bound lies inside the entity's tight box.

use crate::face::box_value;
use crate::geom::Aabb;
use crate::reason::UnavailableReason;
use crate::value::{measured, BoxKind, BoxValue, Errors, Method, Quantity, Unavailable, Unit};

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

/// The union of member boxes, with the first unavailable member.
pub(crate) struct BoxUnion {
    upper: Option<Aabb>,
    lower: Option<Aabb>,
    kind: BoxKind,
    errors: Errors,
    missing: Option<Unavailable<BoxValue>>,
}

fn join(aabb: Option<Aabb>, other: Aabb) -> Aabb {
    aabb.map_or(other, |aabb| aabb.union(other))
}

impl BoxUnion {
    pub(crate) const fn new() -> Self {
        Self {
            upper: None,
            lower: None,
            kind: BoxKind::Tight,
            errors: Errors::ZERO,
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

    /// Add one member's box quantity; `member` names it in the aggregate
    /// reason of an unavailable member.
    pub(crate) fn add(&mut self, member: &str, quantity: &Quantity<BoxValue>) {
        match quantity {
            Quantity::Measured(measured) => {
                let value = measured.value();
                self.include(
                    corners(value),
                    value.kind,
                    lower_bound(value),
                    Errors::new(
                        measured.numeric_error().get(),
                        measured.model_uncertainty().get(),
                    ),
                );
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
        let Some(upper) = self.upper else {
            return Quantity::missing(UnavailableReason::BoundaryUnboundedFace, entity, message);
        };
        let lower = match self.kind {
            BoxKind::Tight => None,
            BoxKind::Conservative => self.lower,
        };
        measured(
            box_value(upper, self.kind, lower),
            Unit::Millimeter,
            Method::Sum,
            self.errors,
            entity.unwrap_or("document"),
        )
    }
}

#[cfg(test)]
mod tests {
    use cadmpeg_ir::math::Point3;

    use super::{lower_bound, BoxUnion};
    use crate::geom::Aabb;
    use crate::value::{BoxKind, Errors};

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

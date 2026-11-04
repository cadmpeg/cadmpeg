// SPDX-License-Identifier: Apache-2.0
//! Compact analytic carriers read from the node sequence of a partition or
//! plain stream (`parasolid.md` §2, `sldprt.md` §7.1).

use super::{
    decode_carrier_values, tag, valid_carrier_frame, valid_carrier_scalars, Carrier,
    MARKER_AGAINST_UNLINKED_EDGES, MARKER_REVERSED,
};
use cadmpeg_parasolid::records::PrimaryRecords;
use std::collections::HashSet;

/// One analytic value field: a vector of three reals or one real.
#[derive(Clone, Copy)]
enum Value {
    Vector(&'static str),
    Real(&'static str),
}

/// The value fields of each analytic node type, in base-schema order after
/// `sense`.
fn value_fields(kind: u8) -> Option<&'static [Value]> {
    use Value::{Real, Vector};
    Some(match kind {
        tag::LINE => &[Vector("pvec"), Vector("direction")],
        tag::CIRCLE => &[
            Vector("centre"),
            Vector("normal"),
            Vector("x_axis"),
            Real("radius"),
        ],
        tag::ELLIPSE => &[
            Vector("centre"),
            Vector("normal"),
            Vector("x_axis"),
            Real("major_radius"),
            Real("minor_radius"),
        ],
        tag::PLANE => &[Vector("pvec"), Vector("normal"), Vector("x_axis")],
        tag::CYLINDER => &[
            Vector("pvec"),
            Vector("axis"),
            Real("radius"),
            Vector("x_axis"),
        ],
        tag::CONE => &[
            Vector("pvec"),
            Vector("axis"),
            Real("radius"),
            Real("sin_half_angle"),
            Real("cos_half_angle"),
            Vector("x_axis"),
        ],
        tag::SPHERE => &[
            Vector("centre"),
            Real("radius"),
            Vector("axis"),
            Vector("x_axis"),
        ],
        tag::TORUS => &[
            Vector("centre"),
            Vector("axis"),
            Real("major_radius"),
            Real("minor_radius"),
            Vector("x_axis"),
        ],
        _ => return None,
    })
}

/// The compact analytic carriers of a node sequence whose body starts at
/// `body_offset`, when every analytic node carries its base fields and no two
/// analytic nodes share an index. Offsets are body-relative. A node whose
/// values fail the frame or scalar checks of its compact form is omitted.
pub(super) fn analytic(records: &PrimaryRecords<'_>, body_offset: usize) -> Option<Vec<Carrier>> {
    let mut carriers = Vec::new();
    let mut indices = HashSet::new();
    for node in records.nodes() {
        let Ok(kind) = u8::try_from(node.kind()) else {
            continue;
        };
        let Some(fields) = value_fields(kind) else {
            continue;
        };
        indices.insert(node.index()).then_some(())?;
        let mut values = Vec::new();
        for field in fields {
            match field {
                Value::Vector(name) => values.extend(node.vector(name)?),
                Value::Real(name) => values.push(node.real(name)?),
            }
        }
        let reversed = match node.char("sense")? {
            MARKER_REVERSED => true,
            MARKER_AGAINST_UNLINKED_EDGES => false,
            _ => continue,
        };
        if values.iter().any(|value| !value.is_finite())
            || !valid_carrier_frame(kind, &values)
            || !valid_carrier_scalars(kind, &values)
        {
            continue;
        }
        let range = node.range();
        let span = (
            range.start.checked_sub(body_offset)?,
            range.end.checked_sub(body_offset)?,
        );
        carriers.extend(decode_carrier_values(
            kind,
            &values,
            node.index(),
            span,
            Some(reversed),
        ));
    }
    Some(carriers)
}

#[cfg(test)]
mod tests;

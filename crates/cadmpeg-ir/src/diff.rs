// SPDX-License-Identifier: Apache-2.0
//! Structural comparison of IR documents.
//!
//! Numbers compare through [`cadmpeg_codec_core::compare`], so a coordinate that
//! differs only in the last place — what the same file decoded under two
//! platforms' libm produces — is not reported as a change, while an integer
//! count, index, or degree that moved by one always is. That module states the
//! tolerance and its caveats, including that the relation is not transitive:
//! every verdict here concerns exactly the two documents passed in.
//!
//! The comparison covers units, tolerances, and every model and native arena. It
//! does not cover [`crate::document::SourceMeta`], so the digest attributes
//! recorded there — `document_local_sha256` and its neighbours — cannot make a diff
//! report a difference. That is the only coherent treatment of a digest here: a
//! digest is a bitwise fingerprint of values this module compares tolerantly, so
//! two decodes that agree to fourteen significant digits hash differently and no
//! tolerance can reconcile them. A caller that needs to compare source metadata
//! must compare it directly and decide for itself which attributes are
//! bit-reproducible.

use std::collections::BTreeMap;

use cadmpeg_codec_core::compare::{floats_agree, values_agree};

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;

use crate::CadIr;

/// A modified entity and its differing top-level fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct ModifiedEntity {
    /// Diff key of the entity, as produced by the arena's key function.
    pub id: String,
    /// Names of the top-level entity fields whose JSON-serialized values differ
    /// between the two documents.
    pub fields: Vec<String>,
}

/// Changes within one entity arena.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct ArenaDiff {
    /// Arena name, matching the field name in [`crate::CadIr`] (e.g. `"faces"`).
    pub kind: String,
    /// Diff keys of entities present only in the right-hand document.
    pub added: Vec<String>,
    /// Diff keys of entities present only in the left-hand document.
    pub removed: Vec<String>,
    /// Entities present in both documents with at least one differing field.
    pub modified: Vec<ModifiedEntity>,
}

/// Structural changes between two IR documents.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct IrDiff {
    /// `(left, right)` units, present only when the two documents' units differ.
    pub unit_change: Option<(crate::units::Units, crate::units::Units)>,
    /// `(left, right)` tolerances, present only when the two documents' tolerances differ.
    pub tolerance_change: Option<(crate::units::Tolerances, crate::units::Tolerances)>,
    /// Per-arena diffs, one entry per arena compared.
    pub per_arena: Vec<ArenaDiff>,
}

impl IrDiff {
    /// Returns `true` when neither units, tolerances, nor any arena differ.
    pub fn is_empty(&self) -> bool {
        self.unit_change.is_none()
            && self.tolerance_change.is_none()
            && self.per_arena.iter().all(|arena| {
                arena.added.is_empty() && arena.removed.is_empty() && arena.modified.is_empty()
            })
    }
}

/// Top-level field names whose values do not agree, tolerating last-place
/// disagreement in fractional numbers at any depth beneath a field.
///
/// A field present on one side and absent on the other always counts as
/// differing, so an `Option` that gained or lost a value is reported even when
/// the value would have agreed.
fn differing_fields<T: Serialize>(left: &T, right: &T) -> Vec<String> {
    let (Ok(Value::Object(left)), Ok(Value::Object(right))) =
        (serde_json::to_value(left), serde_json::to_value(right))
    else {
        return vec!["value".to_string()];
    };
    left.keys()
        .chain(right.keys())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .filter(|key| match (left.get(*key), right.get(*key)) {
            (Some(before), Some(after)) => values_agree(before, after).is_err(),
            (None, None) => false,
            _ => true,
        })
        .cloned()
        .collect()
}

fn arena<T, F>(kind: impl Into<String>, left: &[T], right: &[T], identity: F) -> ArenaDiff
where
    T: PartialEq + Serialize,
    F: for<'a> Fn(&'a T) -> &'a str,
{
    let left: BTreeMap<_, _> = left
        .iter()
        .map(|entity| (identity(entity), entity))
        .collect();
    let right: BTreeMap<_, _> = right
        .iter()
        .map(|entity| (identity(entity), entity))
        .collect();
    let removed = left
        .keys()
        .filter(|id| !right.contains_key(*id))
        .map(|id| (*id).to_owned())
        .collect();
    let added = right
        .keys()
        .filter(|id| !left.contains_key(*id))
        .map(|id| (*id).to_owned())
        .collect();
    let modified = left
        .iter()
        .filter_map(|(id, before)| {
            let after = right.get(id)?;
            // Exact equality is the fast path and skips serializing the pair.
            // Anything else goes to the tolerant field comparison, which decides
            // whether the entity moved at all: an empty field list means every
            // difference was below the tolerance.
            if *before == *after {
                return None;
            }
            let fields = differing_fields(*before, *after);
            (!fields.is_empty()).then(|| ModifiedEntity {
                id: (*id).to_owned(),
                fields,
            })
        })
        .collect();
    ArenaDiff {
        kind: kind.into(),
        added,
        removed,
        modified,
    }
}

macro_rules! define_diff_arenas {
    ($( $field:ident: $element:ty, $doc:literal, [$($attribute:meta),*]; )*) => {
        fn diff_arenas(left: &CadIr, right: &CadIr) -> Vec<ArenaDiff> {
            vec![$(arena(
                stringify!($field),
                &left.model.$field,
                &right.model.$field,
                crate::schema::EntitySchema::identity,
            )),*]
        }
    };
}
crate::document::arena_registry!(define_diff_arenas);

fn diff_native_namespaces(left: &CadIr, right: &CadIr) -> Vec<ArenaDiff> {
    let namespaces = left
        .native
        .0
        .keys()
        .chain(right.native.0.keys())
        .collect::<std::collections::BTreeSet<_>>();
    namespaces
        .into_iter()
        .flat_map(|namespace| {
            let left_ns = left.native.namespace(namespace);
            let right_ns = right.native.namespace(namespace);
            let arenas = left_ns
                .into_iter()
                .flat_map(|value| value.arenas.keys())
                .chain(right_ns.into_iter().flat_map(|value| value.arenas.keys()))
                .collect::<std::collections::BTreeSet<_>>();
            arenas.into_iter().map(move |name| {
                arena(
                    format!("native.{namespace}.{name}"),
                    left_ns
                        .and_then(|value| value.arenas.get(name))
                        .map(Vec::as_slice)
                        .unwrap_or_default(),
                    right_ns
                        .and_then(|value| value.arenas.get(name))
                        .map(Vec::as_slice)
                        .unwrap_or_default(),
                    |record| record.id(),
                )
            })
        })
        .collect()
}

/// Whether two tolerance declarations agree, each component within the
/// comparator's tolerance.
fn tolerances_agree(left: crate::units::Tolerances, right: crate::units::Tolerances) -> bool {
    floats_agree(left.linear, right.linear) && floats_agree(left.angular, right.angular)
}

/// Compare units, tolerances, and every entity arena by stable entity ID.
///
/// Fractional numbers compare within the tolerance stated by
/// [`cadmpeg_codec_core::compare`]; integers, strings, enums, and structure
/// compare exactly.
pub fn diff(left: &CadIr, right: &CadIr) -> IrDiff {
    // `Units` carries only the `LengthUnit` enum, so exact comparison is the
    // correct relation for it; there is no float to tolerate.
    let unit_change =
        (left.units != right.units).then(|| (left.units.clone(), right.units.clone()));
    let tolerance_change = (!tolerances_agree(left.tolerances, right.tolerances))
        .then_some((left.tolerances, right.tolerances));
    let mut per_arena = diff_arenas(left, right);
    per_arena.extend(diff_native_namespaces(left, right));
    IrDiff {
        unit_change,
        tolerance_change,
        per_arena,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::diff;
    use crate::examples::unit_cube;

    #[test]
    fn detects_changes_in_all_document_dimensions() {
        let left = unit_cube();
        let mut right = left.clone();
        right.model.points[0].position.x += 1.0;
        right.model.loops.pop();
        right.model.coedges.pop();

        let result = diff(&left, &right);
        assert!(!result.is_empty());
        assert_eq!(
            result
                .per_arena
                .iter()
                .find(|a| a.kind == "points")
                .expect("required invariant")
                .modified[0]
                .fields,
            ["position"]
        );
        assert_eq!(
            result
                .per_arena
                .iter()
                .find(|a| a.kind == "loops")
                .expect("required invariant")
                .removed
                .len(),
            1
        );
        assert_eq!(
            result
                .per_arena
                .iter()
                .find(|a| a.kind == "coedges")
                .expect("required invariant")
                .removed
                .len(),
            1
        );
    }

    /// Modified entity IDs in one arena of a diff.
    fn modified(result: &super::IrDiff, kind: &str) -> Vec<String> {
        result
            .per_arena
            .iter()
            .find(|arena| arena.kind == kind)
            .expect("every model arena appears in every diff")
            .modified
            .iter()
            .map(|entity| entity.id.clone())
            .collect()
    }

    /// Index of a point whose `x` is at unit magnitude or above, so a relative
    /// perturbation of it is a real perturbation. A zero coordinate would make
    /// either direction of these tests vacuous.
    fn scaled_point(ir: &crate::CadIr) -> usize {
        ir.model
            .points
            .iter()
            .position(|point| point.position.x.abs() >= 1.0)
            .expect("the cube fixture places points away from the origin")
    }

    /// A coordinate moved by one unit in the last place, which is what the same
    /// file decoded under two platforms' libm produces.
    #[test]
    fn a_last_place_coordinate_move_is_not_a_difference() {
        let left = unit_cube();
        let mut right = left.clone();
        let index = scaled_point(&left);
        let before = right.model.points[index].position.x;
        let after = f64::from_bits(before.to_bits() + 1);
        assert_ne!(
            before.to_bits(),
            after.to_bits(),
            "the coordinate must move, or this test proves nothing"
        );
        right.model.points[index].position.x = after;

        assert_ne!(
            serde_json::to_value(&left.model.points).unwrap(),
            serde_json::to_value(&right.model.points).unwrap(),
            "the serialized documents must differ, or exact equality would pass too"
        );
        let result = diff(&left, &right);
        assert!(result.is_empty(), "{result:?}");
    }

    /// A tolerance declaration moved in the last place is the same declaration.
    #[test]
    fn a_last_place_tolerance_move_is_not_a_difference() {
        let left = unit_cube();
        let mut right = left.clone();
        right.tolerances.linear = f64::from_bits(left.tolerances.linear.to_bits() + 1);
        assert!(diff(&left, &right).is_empty());

        right.tolerances.linear = left.tolerances.linear * 2.0;
        assert!(diff(&left, &right).tolerance_change.is_some());
    }

    /// A change with physical meaning stays reported. A relative `1e-6` on a
    /// coordinate is six orders above the tolerance.
    #[test]
    fn a_genuine_coordinate_change_is_still_reported() {
        let left = unit_cube();
        let mut right = left.clone();
        let index = scaled_point(&left);
        let point = &mut right.model.points[index].position;
        point.x = point.x.mul_add(1e-6, point.x);

        let result = diff(&left, &right);
        assert!(!result.is_empty());
        assert_eq!(
            modified(&result, "points"),
            [left.model.points[index].id.0.clone()]
        );
    }

    /// An integer field is never tolerated, however large its magnitude and
    /// however small the change relative to it.
    #[test]
    fn an_integer_field_differing_by_one_is_always_reported() {
        use crate::geometry::{Curve, CurveGeometry, NurbsCurve};
        use crate::ids::CurveId;
        use crate::math::Point3;

        let nurbs = |degree: u32| Curve {
            id: CurveId("synthetic:tolerance:curve#nurbs".into()),
            geometry: CurveGeometry::Nurbs(NurbsCurve {
                degree,
                knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                control_points: vec![
                    Point3::new(0.0, 0.0, 0.0),
                    Point3::new(1.0, 0.0, 0.0),
                    Point3::new(2.0, 0.0, 0.0),
                ],
                weights: None,
                periodic: false,
            }),
            source_object: None,
        };

        let mut left = unit_cube();
        let mut right = left.clone();
        left.model.curves.push(nurbs(2));
        right.model.curves.push(nurbs(3));

        let result = diff(&left, &right);
        assert!(!result.is_empty());
        assert_eq!(
            modified(&result, "curves"),
            ["synthetic:tolerance:curve#nurbs"]
        );
    }

    /// `source` metadata, including the digest attributes, is outside what this
    /// comparison covers, so a digest that cannot agree across platforms cannot
    /// make a diff report a difference either.
    #[test]
    fn source_metadata_is_outside_the_comparison() {
        let left = unit_cube();
        let mut right = left.clone();
        let source = right
            .source
            .get_or_insert_with(|| crate::document::SourceMeta {
                format: "synthetic".into(),
                ..Default::default()
            });
        source
            .attributes
            .insert("document_local_sha256".into(), "0".repeat(64));

        assert_ne!(left.source, right.source);
        assert!(diff(&left, &right).is_empty());
    }

    #[test]
    fn identical_documents_have_empty_diff() {
        let ir = unit_cube();
        assert!(diff(&ir, &ir).is_empty());
    }
}

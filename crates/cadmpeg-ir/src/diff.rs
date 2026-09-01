// SPDX-License-Identifier: Apache-2.0
//! Structural comparison of IR documents.
//!
//! Numbers compare through [`cadmpeg_ir::compare`], so a coordinate that
//! differs only in the last place — what the same file decoded under two
//! platforms' libm produces — is not reported as a change, while an integer
//! count, index, or degree that moved by one always is. That module states the
//! tolerance and its caveats, including that the relation is not transitive:
//! every verdict here concerns exactly the two documents passed in.
//!
//! The comparison covers units, tolerances, every model and native arena, and
//! [`crate::document::SourceMeta`] — the source format id, all dialect layers,
//! and its attributes, where a codec records the program version, the object
//! count, and the rest of what it read out of the container.
//!
//! One class of attribute is carved out. A machine-local digest, named by the
//! [`cadmpeg_ir::compare::LOCAL_DIGEST_SUFFIX`] convention, is a bitwise
//! fingerprint of the very values this module compares tolerantly: two decodes
//! that agree to fourteen significant digits hash differently, and no tolerance
//! can reconcile them. Reporting such a difference as a difference would make
//! every cross-platform comparison of one file report that file as changed.
//! [`SourceDiff::local_digests`] therefore reports them for information, outside
//! [`IrDiff::is_empty`] and outside the exit code derived from it, while every
//! other source attribute counts.
//!
//! A document with no `source` compares as one whose source is
//! [`SourceMeta::default`]: an empty format id and no attributes. Two documents
//! that both lack source metadata therefore agree, and a document that gained a
//! populated one differs.

use std::collections::BTreeMap;

use crate::compare::{floats_agree, is_local_digest_attribute, values_agree};
use cadmpeg_core::dialect::DialectLayers;

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;

use crate::document::SourceMeta;
use crate::CadIr;

/// One differing source attribute.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct AttributeChange {
    /// Attribute key.
    pub key: String,
    /// Value in the left-hand document, absent when only the right-hand document
    /// carries the key.
    pub left: Option<String>,
    /// Value in the right-hand document, absent when only the left-hand document
    /// carries the key.
    pub right: Option<String>,
}

/// Changes in the two documents' source metadata.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SourceDiff {
    /// `(left, right)` source format ids, present only when they differ.
    pub format_change: Option<(String, String)>,
    /// `(left, right)` complete dialect-layer sets, present when they differ.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dialects_change: Option<(Option<DialectLayers>, Option<DialectLayers>)>,
    /// Differing attributes, each a difference.
    pub attributes: Vec<AttributeChange>,
    /// Differing machine-local digest attributes, reported for information and
    /// never counted as a difference.
    ///
    /// See the module documentation: such a digest cannot agree across platforms,
    /// so a verdict that turned on one would call the same file changed.
    pub local_digests: Vec<AttributeChange>,
}

impl SourceDiff {
    /// Returns `true` when nothing that counts as a difference changed.
    ///
    /// [`Self::local_digests`] is not consulted.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.format_change.is_none() && self.dialects_change.is_none() && self.attributes.is_empty()
    }
}

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
    /// Source-metadata changes, including the informational digest section.
    pub source: SourceDiff,
    /// Per-arena diffs, one entry per arena compared.
    pub per_arena: Vec<ArenaDiff>,
}

impl IrDiff {
    /// Returns `true` when neither units, tolerances, source metadata, nor any
    /// arena differ.
    ///
    /// A difference confined to [`SourceDiff::local_digests`] leaves this `true`.
    pub fn is_empty(&self) -> bool {
        self.unit_change.is_none()
            && self.tolerance_change.is_none()
            && self.source.is_empty()
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
            // Empty differing_fields means every difference was below tolerance.
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

/// Compare the source metadata of two documents, classifying each differing
/// attribute as a difference or as an informational machine-local digest.
fn diff_source(left: &CadIr, right: &CadIr) -> SourceDiff {
    let absent = SourceMeta::default();
    let left = left.source.as_ref().unwrap_or(&absent);
    let right = right.source.as_ref().unwrap_or(&absent);
    let mut result = SourceDiff {
        format_change: (left.format() != right.format())
            .then(|| (left.format().to_owned(), right.format().to_owned())),
        dialects_change: (left.dialects() != right.dialects())
            .then(|| (left.dialects().cloned(), right.dialects().cloned())),
        ..SourceDiff::default()
    };
    for change in attribute_changes(&left.attributes, &right.attributes) {
        if is_local_digest_attribute(&change.key) {
            result.local_digests.push(change);
        } else {
            result.attributes.push(change);
        }
    }
    result
}

/// Compare two string maps by key, reporting one change per differing key in
/// key order.
fn attribute_changes(
    left: &BTreeMap<String, String>,
    right: &BTreeMap<String, String>,
) -> Vec<AttributeChange> {
    left.keys()
        .chain(right.keys())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .filter_map(|key| {
            let before = left.get(key);
            let after = right.get(key);
            (before != after).then(|| AttributeChange {
                key: key.clone(),
                left: before.cloned(),
                right: after.cloned(),
            })
        })
        .collect()
}

/// Compare units, tolerances, source metadata, and every entity arena by stable
/// entity ID.
///
/// Fractional numbers compare within the tolerance stated by
/// [`cadmpeg_ir::compare`]; integers, strings, enums, and structure
/// compare exactly. Source attributes are strings and compare exactly; a
/// machine-local digest among them is reported without counting as a difference.
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
        source: diff_source(left, right),
        per_arena,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::collections::BTreeMap;

    use super::diff;
    use crate::compare;
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

    /// Index of a point whose `x` is at unit magnitude or above.
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

    /// A change with physical meaning stays reported. A relative `1.0e-6` on a
    /// coordinate is six orders above the tolerance.
    #[test]
    fn a_genuine_coordinate_change_is_still_reported() {
        let left = unit_cube();
        let mut right = left.clone();
        let index = scaled_point(&left);
        let point = &mut right.model.points[index].position;
        point.x = point.x.mul_add(1.0e-6, point.x);

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

    /// A cube carrying source metadata with the given attributes.
    fn with_source(attributes: &[(&str, &str)]) -> crate::CadIr {
        let mut ir = unit_cube();
        ir.source = Some(crate::document::SourceMeta::unclassified(
            "rhino",
            attributes
                .iter()
                .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
                .collect(),
        ));
        ir
    }

    fn classify_source(ir: &mut crate::CadIr, dialect: cadmpeg_core::dialect::DialectMatch) {
        let source = ir
            .source
            .take()
            .expect("the test document has source metadata");
        ir.source = Some(crate::document::SourceMeta::classified(
            cadmpeg_core::dialect::DialectLayers::of(dialect),
            source.attributes,
        ));
    }

    #[test]
    fn a_source_attribute_difference_is_a_difference() {
        let left = with_source(&[("program_version", "1.0"), ("object_count", "3")]);
        let right = with_source(&[("program_version", "1.1"), ("object_count", "3")]);

        let result = diff(&left, &right);
        assert!(!result.is_empty());
        assert_eq!(
            result.source.attributes,
            [super::AttributeChange {
                key: "program_version".to_owned(),
                left: Some("1.0".to_owned()),
                right: Some("1.1".to_owned()),
            }]
        );
        assert!(result.source.local_digests.is_empty());
    }

    /// An attribute present on one side only is a difference in either direction.
    #[test]
    fn an_added_or_removed_source_attribute_is_a_difference() {
        let left = with_source(&[("object_count", "3")]);
        let right = with_source(&[]);

        let forward = diff(&left, &right);
        assert!(!forward.is_empty());
        assert_eq!(forward.source.attributes[0].left.as_deref(), Some("3"));
        assert_eq!(forward.source.attributes[0].right, None);

        let backward = diff(&right, &left);
        assert!(!backward.is_empty());
        assert_eq!(backward.source.attributes[0].left, None);
        assert_eq!(backward.source.attributes[0].right.as_deref(), Some("3"));
    }

    /// A machine-local digest is a bitwise fingerprint of tolerantly compared
    /// values, so two platforms' decodes of one file disagree on it while
    /// agreeing on the model. Such a difference is reported and does not make the
    /// documents different.
    #[test]
    fn a_machine_local_digest_difference_is_reported_but_is_not_a_difference() {
        let left = with_source(&[("document_local_sha256", &"0".repeat(64))]);
        let right = with_source(&[("document_local_sha256", &"1".repeat(64))]);

        let result = diff(&left, &right);
        assert_ne!(left.source, right.source);
        assert!(result.is_empty(), "{result:?}");
        assert!(result.source.attributes.is_empty());
        assert_eq!(
            result
                .source
                .local_digests
                .iter()
                .map(|change| change.key.as_str())
                .collect::<Vec<_>>(),
            ["document_local_sha256"]
        );
    }

    /// Carve-out keys by `_local_sha256` suffix, not a fixed key list.
    #[test]
    fn the_carve_out_follows_the_suffix_convention() {
        let key = format!("future_codec_thing{}", compare::LOCAL_DIGEST_SUFFIX);
        let left = with_source(&[(&key, "a"), ("footer_fingerprint", "f")]);
        let right = with_source(&[(&key, "b"), ("footer_fingerprint", "f")]);
        let result = diff(&left, &right);
        assert!(result.is_empty(), "{result:?}");
        assert_eq!(result.source.local_digests[0].key, key);

        // Digests over retained source bytes have no suffix; a change stays a difference.
        let right = with_source(&[(&key, "b"), ("footer_fingerprint", "g")]);
        let result = diff(&left, &right);
        assert!(!result.is_empty());
        assert_eq!(result.source.attributes[0].key, "footer_fingerprint");
    }

    /// A document with no source metadata compares against one that has some
    /// without panicking, in either order, and an absent source equals an empty
    /// one.
    #[test]
    fn absent_source_metadata_compares_without_panicking() {
        let mut bare = unit_cube();
        bare.source = None;
        let populated = with_source(&[("object_count", "3")]);

        for (left, right) in [(&bare, &populated), (&populated, &bare)] {
            let result = diff(left, right);
            assert!(!result.is_empty());
            assert!(result.source.format_change.is_some());
            assert_eq!(result.source.attributes.len(), 1);
        }

        let mut empty_source = unit_cube();
        empty_source.source = Some(crate::document::SourceMeta::default());
        assert!(diff(&bare, &empty_source).is_empty());
    }

    #[test]
    fn identical_documents_have_empty_diff() {
        let ir = unit_cube();
        assert!(diff(&ir, &ir).is_empty());
    }

    /// The dialect and its declared fields are compared, so a divergence there
    /// cannot pass as agreement.
    #[test]
    fn a_dialect_or_declared_divergence_is_a_difference() {
        let mut left = with_source(&[]);
        let mut right = left.clone();
        classify_source(
            &mut left,
            cadmpeg_core::dialect::DialectMatch::admitted(
                cadmpeg_core::dialect::DialectId::pinned("rhino:archive-70"),
            ),
        );
        classify_source(
            &mut right,
            cadmpeg_core::dialect::DialectMatch::admitted(
                cadmpeg_core::dialect::DialectId::pinned("rhino:archive-80"),
            ),
        );

        let result = diff(&left, &right);
        assert!(!result.is_empty());
        assert_eq!(
            result.source.dialects_change,
            Some((
                left.source.as_ref().unwrap().dialects().cloned(),
                right.source.as_ref().unwrap().dialects().cloned(),
            ))
        );

        let mut declared_left = with_source(&[]);
        let mut declared_right = declared_left.clone();
        classify_source(
            &mut declared_left,
            cadmpeg_core::dialect::DialectMatch::admitted(
                cadmpeg_core::dialect::DialectId::pinned("rhino:archive-70"),
            )
            .with_declared(BTreeMap::from([("archive_version".into(), "70".into())])),
        );
        classify_source(
            &mut declared_right,
            cadmpeg_core::dialect::DialectMatch::admitted(
                cadmpeg_core::dialect::DialectId::pinned("rhino:archive-70"),
            )
            .with_declared(BTreeMap::from([("archive_version".into(), "80".into())])),
        );

        let declared = diff(&declared_left, &declared_right);
        assert!(!declared.is_empty());
        assert_eq!(
            declared.source.dialects_change,
            Some((
                declared_left.source.as_ref().unwrap().dialects().cloned(),
                declared_right.source.as_ref().unwrap().dialects().cloned(),
            ))
        );
        assert!(declared.source.attributes.is_empty());
    }

    #[test]
    fn admission_and_instance_divergence_are_differences() {
        use cadmpeg_core::dialect::{DialectId, DialectLayers, DialectMatch};

        let mut left = with_source(&[]);
        let mut right = left.clone();
        classify_source(
            &mut left,
            DialectMatch::admitted(DialectId::pinned("rhino:archive-80")),
        );
        classify_source(
            &mut right,
            DialectMatch::refused(DialectId::pinned("rhino:archive-80")),
        );
        assert!(!diff(&left, &right).is_empty());

        classify_source(
            &mut right,
            DialectMatch::admitted(DialectId::pinned("rhino:archive-80"))
                .with_instance("embedded/model.3dm"),
        );
        assert!(!diff(&left, &right).is_empty());

        let source = right.source.take().unwrap();
        right.source = Some(crate::document::SourceMeta::classified(
            DialectLayers::new(
                DialectMatch::admitted(DialectId::pinned("rhino:archive-80")),
                vec![DialectMatch::residual(DialectId::pinned("acis:text-acis"))
                    .with_instance("body.sat")],
            )
            .unwrap(),
            source.attributes,
        ));
        let result = diff(&left, &right);
        assert!(!result.is_empty());
        assert_eq!(
            result
                .source
                .dialects_change
                .as_ref()
                .unwrap()
                .1
                .as_ref()
                .unwrap()
                .iter()
                .count(),
            2
        );
    }

    /// The staged fields add nothing to the serialized diff while they are
    /// empty, which is what keeps this output stable across the migration.
    #[test]
    fn an_unpopulated_dialect_adds_no_key_to_the_serialized_diff() {
        let ir = with_source(&[]);
        let rendered = serde_json::to_string(&diff(&ir, &ir).source).unwrap();

        assert!(!rendered.contains("dialects_change"), "{rendered}");
    }
}

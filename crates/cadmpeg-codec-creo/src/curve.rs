// SPDX-License-Identifier: Apache-2.0
//! Curve namespace prototypes and topology rows.
//!
//! Prototype rows identify curves and their generating features. Topology rows
//! add the two face sides and successor curve for each native half-edge. Curve
//! parameter bodies are not interpreted here.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::cursor::bounded_len;

use crate::psb::{self, compact_int, reference_id};
use crate::scalar;

/// A labeled curve namespace entry.
///
/// `type_byte` remains raw because the namespace grammar does not define its
/// geometric interpretation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurvePrototype {
    /// The row's `crv_id`: the curve's identifier in the `crv_array`
    /// namespace, referenced by `srf_array` and topology row `E0`/`E1`
    /// fields.
    pub id: u32,
    /// The row's raw `type` byte. Its geometric meaning is not identified by
    /// the namespace grammar alone ([spec §4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#4-curve-namespace-crv_array)); the curve-body evaluator
    /// determines the interpretation.
    pub type_byte: u8,
    /// The `feat_id` compact integer, when the labeled row has one: the
    /// feature that generated this curve.
    pub feature_id: Option<u32>,
    /// Byte offset of this prototype's `crv_array` label in the original
    /// stream.
    pub offset: usize,
}

/// One source line in a curve-equation expression program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurveExpressionLine {
    /// UTF-8 source text without its NUL terminator.
    pub text: String,
    /// Byte offset of the first source byte.
    pub offset: usize,
}

/// Expression program stored by a curve-from-equation entity.
#[derive(Debug, Clone, PartialEq)]
pub struct CurveExpressionRecord {
    /// Entity identifier from the enclosing record.
    pub entity_id: u32,
    /// Whether the enclosing record is `backup_ents(crv_fr_eqn)`.
    pub backup: bool,
    /// Bounded native placement frame carried by the equation entity.
    pub local_system: Option<CurveExpressionLocalSystem>,
    /// Ordered source lines declared by the `f8` array.
    pub lines: Vec<CurveExpressionLine>,
    /// Assignment statements in source order.
    pub assignments: Vec<CurveExpressionAssignment>,
    /// Byte offset of the enclosing entity label.
    pub offset: usize,
    /// Byte offset of the `expression` field.
    pub expression_offset: usize,
}

/// Count-bounded `local_sys` payload carried by a curve-equation entity.
#[derive(Debug, Clone, PartialEq)]
pub struct CurveExpressionLocalSystem {
    /// Tuple dimensionality from the `f9` wrapper.
    pub dimensions: u32,
    /// Stored tuple count from the `f9` wrapper.
    pub count: u32,
    /// Exact stateful scalar body through the next named field.
    pub body: Vec<u8>,
    /// Twelve explicit scalar slots, absent when the body uses inheritance or
    /// contains a scalar form that is not decoded.
    pub explicit_slots: Option<[f64; 12]>,
    /// Byte offset of the `local_sys` named-record header.
    pub offset: usize,
}

/// One executable assignment in a curve expression program.
#[derive(Debug, Clone, PartialEq)]
pub struct CurveExpressionAssignment {
    /// Assigned identifier.
    pub name: String,
    /// Exact right-hand expression after surrounding ASCII whitespace removal.
    pub expression: String,
    /// Referenced identifiers in first-appearance order.
    pub dependencies: Vec<String>,
    /// Sequentially evaluated value when every dependency is resolved.
    pub value: Option<CurveExpressionValue>,
    /// Whether the source-ordered conditional program executes this assignment.
    pub activation: CurveExpressionActivation,
    /// Byte offset of the assignment source line.
    pub offset: usize,
}

/// A deterministic value produced by a curve relation expression.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(untagged)]
pub enum CurveExpressionValue {
    /// Dimensionless numeric value.
    Number(f64),
    /// UTF-8 string value.
    String(String),
}

impl CurveExpressionValue {
    fn truth(&self) -> Option<bool> {
        match self {
            Self::Number(value) => Some(*value != 0.0),
            Self::String(_) => None,
        }
    }
}

/// Evaluation state of an assignment inside relation conditionals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurveExpressionActivation {
    /// The assignment executes in the current source-ordered evaluation.
    Active,
    /// A resolved enclosing condition excludes the assignment.
    Inactive,
    /// An enclosing condition cannot be evaluated from available scalar values.
    Conditional,
}

impl CurveExpressionActivation {
    pub(crate) const fn token(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Inactive => "inactive",
            Self::Conditional => "conditional",
        }
    }
}

/// Exact cylindrical helix parameters from a `crv_fr_eqn` program.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurveExpressionHelix {
    /// Constant cylindrical radius in model millimeters.
    pub radius: f64,
    /// Signed axial rise from `t = 0` through `t = 1`.
    pub height: f64,
    /// Native axial coordinate at `t = 0`.
    pub z_start: f64,
    /// Positive angular travel in revolutions.
    pub revolutions: f64,
    /// Angular position at `t = 0`, in radians.
    pub start_angle: f64,
    /// Whether angular travel decreases as `t` increases.
    pub clockwise: bool,
}

/// A curve row with a uniquely delimited topology suffix.
///
/// `faces` and `next_edges` preserve the two native sides in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurveTopologyRow {
    /// The row's `crv_id`, matching a [`CurvePrototype::id`] in the same
    /// `crv_array` namespace.
    pub id: u32,
    /// The row's raw `type` byte; see [`CurvePrototype::type_byte`].
    pub type_byte: u8,
    /// The `feat_id` compact integer: the feature that generated this
    /// curve.
    pub feature_id: u32,
    /// The two `crv_pnt_dir` orientation-flag bytes, one per half-edge side.
    /// These are per-side orientation flags, not a tangent vector
    /// ([spec §4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#4-curve-namespace-crv_array)).
    pub directions: [u8; 2],
    /// The `F0`/`F1` suffix fields: the `srf_array` face identifiers
    /// bounding the curve's two half-edge sides.
    pub faces: [u32; 2],
    /// The `E0`/`E1` suffix fields: the `crv_array` identifier of the next
    /// edge for each of the two half-edge sides, used to walk loops
    /// ([spec §4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#4-curve-namespace-crv_array)).
    pub next_edges: [u32; 2],
    /// Byte offset of the row's `crv_id` field in the original stream.
    pub offset: usize,
}

/// One DEPDB cross-section curve row with its one-sided topology suffix.
#[derive(Debug, Clone, PartialEq)]
pub struct DepdbCurveRow {
    /// Curve identifier in the cross-section `crv_array` namespace.
    pub id: u32,
    /// Raw curve-family discriminator.
    pub type_byte: u8,
    /// Owning feature identifier.
    pub feature_id: u32,
    /// Stored per-side direction flags.
    pub directions: [u8; 2],
    /// The `[0, X1, F1, 0]` one-sided suffix.
    pub suffix: [u32; 4],
    /// Exact bytes between the fixed prefix and one-sided suffix.
    pub body: Vec<u8>,
    /// Decoded scalar tokens with exact body-relative spans.
    pub scalar_tokens: Vec<CurveParameterScalar>,
    /// Canonical entity references with exact body-relative spans.
    pub references: Vec<CurveParameterReference>,
    /// Maximal body spans not claimed by a scalar or reference token.
    pub opaque_spans: Vec<CurveParameterOpaqueSpan>,
    /// Byte offset of the row identifier.
    pub offset: usize,
}

/// Resolution state of a curve row's four-reference topology suffix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurveSuffixStatus {
    /// Exactly one canonical suffix boundary exists.
    Unique,
    /// Multiple canonical suffix boundaries exist; connectivity is withheld.
    Ambiguous {
        /// Number of byte-valid suffix boundaries.
        candidate_count: usize,
    },
}

/// Bounded analytic parameter body from one positional `crv_array` row.
#[derive(Debug, Clone, PartialEq)]
pub struct CurveParameterRecord {
    /// Owning curve identifier.
    pub curve_id: u32,
    /// Raw curve-family discriminator.
    pub type_byte: u8,
    /// Exact bytes between direction flags and the selected suffix boundary.
    pub body: Vec<u8>,
    /// Decoded scalar values in byte order.
    pub scalar_values: Vec<f64>,
    /// Scalar tokens with exact body-relative spans.
    pub scalar_tokens: Vec<CurveParameterScalar>,
    /// Canonical entity references skipped while walking the scalar lane.
    pub skipped_references: Vec<u32>,
    /// Canonical entity references with exact body-relative spans.
    pub references: Vec<CurveParameterReference>,
    /// Maximal byte spans not claimed by scalar or reference tokens.
    pub opaque_spans: Vec<CurveParameterOpaqueSpan>,
    /// Whether the topology suffix boundary is unique.
    pub suffix: CurveSuffixStatus,
    /// Byte offset of the positional row in the original stream.
    pub offset: usize,
    /// Byte offset of the first parameter-body byte in the original stream.
    pub body_offset: usize,
    /// Byte offset of the selected body/suffix boundary in the original stream.
    pub suffix_offset: usize,
}

/// One decoded scalar token in a positional curve body.
#[derive(Debug, Clone, PartialEq)]
pub struct CurveParameterScalar {
    /// Decoded scalar value.
    pub value: f64,
    /// Exact token bytes.
    pub raw: Vec<u8>,
    /// Body-relative token offset.
    pub offset: usize,
    /// Token length in bytes.
    pub length: usize,
}

/// One canonical entity reference in a positional curve body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurveParameterReference {
    /// Referenced entity identifier.
    pub entity_id: u32,
    /// Body-relative reference-token offset, including `f7`.
    pub offset: usize,
    /// Reference-token length in bytes, including `f7`.
    pub length: usize,
}

/// One maximal unclaimed byte span in a positional curve body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurveParameterOpaqueSpan {
    /// Exact unclaimed bytes.
    pub raw: Vec<u8>,
    /// Body-relative span offset.
    pub offset: usize,
    /// Span length in bytes.
    pub length: usize,
}

/// Two pcurve endpoints represented in both adjacent face parameter frames.
#[derive(Debug, Clone, PartialEq)]
pub struct PcurveEndpoints {
    /// Owning curve identifier.
    pub curve_id: u32,
    /// Adjacent face identifiers corresponding to face frames zero and one.
    pub faces: [u32; 2],
    /// Endpoint A then B in the first face's local UV frame.
    pub face_0_endpoints: [[f64; 2]; 2],
    /// Endpoint A then B in the second face's local UV frame.
    pub face_1_endpoints: [[f64; 2]; 2],
    /// Byte offset of the source positional curve row.
    pub offset: usize,
}

/// Ordered world-coordinate lane from an `fc <subtype>` dense curve body.
#[derive(Debug, Clone, PartialEq)]
pub struct FcCurveCoordinates {
    /// Owning curve identifier.
    pub curve_id: u32,
    /// Byte following the `fc` body prefix.
    pub subtype: u8,
    /// Exact complete curve parameter body, including the `fc` prefix.
    pub body: Vec<u8>,
    /// Ordered exact world-coordinate values, in mm.
    pub values_mm: Vec<f64>,
    /// World-coordinate tokens with exact body-relative spans.
    pub tokens: Vec<FcCurveCoordinateToken>,
    /// Maximal body spans not owned by a recognized coordinate token.
    pub opaque_spans: Vec<FcCurveOpaqueSpan>,
    /// Byte offset of the source positional curve row.
    pub offset: usize,
}

/// One recognized world-coordinate token in an `fc <subtype>` body.
#[derive(Debug, Clone, PartialEq)]
pub struct FcCurveCoordinateToken {
    /// Decoded model length in millimeters.
    pub value_mm: f64,
    /// Exact source bytes occupied by the token.
    pub raw: Vec<u8>,
    /// Token offset relative to the complete curve parameter body.
    pub offset: usize,
    /// Number of source bytes occupied by the token.
    pub length: usize,
}

/// One maximal unclaimed span in an `fc <subtype>` body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FcCurveOpaqueSpan {
    /// Exact source bytes in the span.
    pub raw: Vec<u8>,
    /// Span offset relative to the complete curve parameter body.
    pub offset: usize,
    /// Number of source bytes in the span.
    pub length: usize,
}

/// Circle proven by the decoded points of an `fc 05` curve body.
#[derive(Debug, Clone, PartialEq)]
pub struct Fc05Circle {
    /// Owning curve identifier.
    pub curve_id: u32,
    /// Circle center in the FC row's in-plane coordinate frame.
    pub center_row_frame: [f64; 2],
    /// Exact radius in mm.
    pub radius_mm: f64,
    /// Unit radial direction from the fitted center to the first stored sample.
    pub sample_direction_row_frame: [f64; 2],
    /// Unit radial direction at stored curve parameter zero in the row's
    /// `(x, z)` frame.
    pub reference_direction_row_frame: Option<[f64; 2]>,
    /// Signed relation from stored parameter to row-frame polar angle.
    /// `1` increases polar angle and `-1` decreases it.
    pub parameter_sign: Option<i8>,
    /// Constant cap-plane ordinate when present in every point.
    pub cap_ordinate_row_frame: Option<f64>,
    /// Number of points participating in validation.
    pub point_count: usize,
    /// Maximum absolute radial residual.
    pub max_residual: f64,
    /// Whether stored parameters match angular deltas around the circle.
    pub angle_parameter_consistent: bool,
    /// Byte offset of the source positional curve row.
    pub offset: usize,
}

/// Two or more topology-bound `fc 05` cap circles that establish one native
/// cylinder's radius and row-frame axis line, but not its model-space frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Fc05CylinderCapPair {
    /// Cylinder surface identifier shared by every cap edge.
    pub surface_id: u32,
    /// Curve identifiers of the agreeing cap circles in source order.
    pub curve_ids: Vec<u32>,
    /// Plane surface identifier opposite the cylinder on each cap edge.
    pub cap_plane_ids: Vec<u32>,
    /// Cap ordinate aligned with each `curve_ids`/`cap_plane_ids` entry.
    pub curve_cap_ordinates_row_frame: Vec<f64>,
    /// Shared center in the owning feature's row frame.
    pub center_row_frame: [f64; 2],
    /// Shared exact radius in mm.
    pub radius_mm: f64,
    /// Unit radial direction at parameter zero in the row's `(x, z)` frame.
    pub reference_direction_row_frame: [f64; 2],
    /// Shared signed parameter-to-polar-angle relation.
    pub parameter_sign: i8,
    /// At least two distinct cap ordinates in the owning feature's row frame.
    pub cap_ordinates_row_frame: Vec<f64>,
    /// Byte offset of the first participating curve row.
    pub offset: usize,
}

/// Complete eight-slot pcurve endpoints from a labeled curve prototype.
#[derive(Debug, Clone, PartialEq)]
pub struct PrototypePcurveEndpoints {
    /// Prototype curve identifier.
    pub curve_id: u32,
    /// Endpoint A then B in schema face frame zero.
    pub face_0_endpoints: [[f64; 2]; 2],
    /// Endpoint A then B in schema face frame one.
    pub face_1_endpoints: [[f64; 2]; 2],
    /// Byte offset of the `crv_pnt_arr` label in the original stream.
    pub offset: usize,
}

/// Four labeled topology references of a curve prototype.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurvePrototypeTopology {
    /// Prototype curve identifier.
    pub curve_id: u32,
    /// Adjacent surface identifiers from `crv_hdr_geom_ptr[0/1]`.
    pub faces: [u32; 2],
    /// Per-face successor curve identifiers from `next_crv_hdr_ptr[0/1]`.
    pub next_edges: [u32; 2],
    /// Byte offset of the prototype namespace.
    pub offset: usize,
}

/// Prototype pcurve endpoints bound to their two labeled adjacent faces.
#[derive(Debug, Clone, PartialEq)]
pub struct BoundPrototypePcurve {
    /// Prototype curve identifier.
    pub curve_id: u32,
    /// Adjacent face identifiers corresponding to UV frames zero and one.
    pub faces: [u32; 2],
    /// Endpoint A then B in the first face's UV frame.
    pub face_0_endpoints: [[f64; 2]; 2],
    /// Endpoint A then B in the second face's UV frame.
    pub face_1_endpoints: [[f64; 2]; 2],
    /// Byte offset of the source prototype pcurve.
    pub offset: usize,
}

/// Discover every labeled `crv_array` prototype. A label range ends at the
/// following `crv_array` label, so DEPDB-concatenated namespaces remain
/// independent.
pub fn prototypes(payload: &[u8]) -> Vec<CurvePrototype> {
    let mut result = Vec::new();
    let mut start = 0;
    while let Some(relative) = find(payload, b"crv_array\0", start) {
        let section_start = relative;
        start = relative + b"crv_array\0".len();
        let section_end = find(payload, b"crv_array\0", start).unwrap_or(payload.len());
        let Some(id_label) = find_in(payload, b"crv_id\0", start, section_end) else {
            continue;
        };
        let id_start = id_label + b"crv_id\0".len();
        let (id, id_end) = compact_int(payload, id_start);
        if id_end == id_start {
            continue;
        }
        let Some(type_label) = find_in(payload, b"type\0", id_end, section_end) else {
            continue;
        };
        let Some(&type_byte) = payload.get(type_label + b"type\0".len()) else {
            continue;
        };
        let feature_id = find_in(payload, b"feat_id\0", id_end, section_end).and_then(|label| {
            let value_start = label + b"feat_id\0".len();
            let (value, end) = compact_int(payload, value_start);
            (end != value_start).then_some(value)
        });
        result.push(CurvePrototype {
            id,
            type_byte,
            feature_id,
            offset: section_start,
        });
    }
    result
}

/// Decode bounded curve-from-equation expression programs.
pub fn expression_records(payload: &[u8]) -> Vec<CurveExpressionRecord> {
    expression_records_with_model_name(payload, None)
}

/// Decode curve-expression programs with an unambiguous current-model name.
pub(crate) fn expression_records_with_model_name(
    payload: &[u8],
    model_name: Option<&str>,
) -> Vec<CurveExpressionRecord> {
    const PRIMARY: &[u8] = b"entity(crv_fr_eqn)\0";
    const BACKUP: &[u8] = b"backup_ents(crv_fr_eqn)\0";
    const ID: &[u8] = b"\xe0\x01id\0";
    const EXPRESSION: &[u8] = b"\xe0\x0aexpression\0";
    const LOCAL_SYSTEM: &[u8] = b"\xe0\x02local_sys\0\xf9";

    let mut labels = Vec::new();
    for (label, backup) in [(PRIMARY, false), (BACKUP, true)] {
        let mut start = 0;
        while let Some(offset) = find(payload, label, start) {
            labels.push((offset, label.len(), backup));
            start = offset + label.len();
        }
    }
    labels.sort_unstable_by_key(|(offset, _, _)| *offset);

    let cache = scalar::ScalarCache::from_section(payload);
    let mut records = Vec::new();
    for (index, &(offset, label_len, backup)) in labels.iter().enumerate() {
        let end = labels
            .get(index + 1)
            .map_or(payload.len(), |(next, _, _)| *next);
        let Some(id_label) = find_in(payload, ID, offset + label_len, end) else {
            continue;
        };
        let id_start = id_label + ID.len();
        let (entity_id, after_id) = compact_int(payload, id_start);
        if after_id == id_start {
            continue;
        }
        let local_system = find_in(payload, LOCAL_SYSTEM, after_id, end).and_then(|offset| {
            let extents_start = offset + LOCAL_SYSTEM.len();
            let (dimensions, dimensions_end) = compact_int(payload, extents_start);
            let (count, body_start) = compact_int(payload, dimensions_end);
            (dimensions_end > extents_start && body_start > dimensions_end && body_start <= end)
                .then_some(())?;
            let body_end = payload[body_start..end]
                .windows(1)
                .position(|window| window[0] == psb::token::NAMED_RECORD)
                .map_or(end, |relative| body_start + relative);
            let body = payload[body_start..body_end].to_vec();
            Some(CurveExpressionLocalSystem {
                dimensions,
                count,
                explicit_slots: ((dimensions, count) == (4, 3))
                    .then(|| scalar::decode_explicit_local_system_slots(&body, &cache))
                    .flatten(),
                body,
                offset,
            })
        });
        let Some(expression_offset) = find_in(payload, EXPRESSION, after_id, end) else {
            continue;
        };
        let opener = expression_offset + EXPRESSION.len();
        if payload.get(opener) != Some(&psb::token::ARRAY_OPEN) {
            continue;
        }
        let (count, mut cursor) = compact_int(payload, opener + 1);
        if cursor == opener + 1 || cursor > end {
            continue;
        }
        let mut lines = Vec::new();
        for _ in 0..count {
            let Some(relative_end) = payload[cursor..end].iter().position(|byte| *byte == 0) else {
                lines.clear();
                break;
            };
            let line_end = cursor + relative_end;
            let Ok(text) = std::str::from_utf8(&payload[cursor..line_end]) else {
                lines.clear();
                break;
            };
            lines.push(CurveExpressionLine {
                text: text.to_owned(),
                offset: cursor,
            });
            cursor = line_end + 1;
        }
        if lines.len() == usize::try_from(count).unwrap_or(usize::MAX) {
            let assignments = evaluate_expression_program(
                &lines,
                model_name,
                &ExternalRelationSymbols::default(),
            );
            records.push(CurveExpressionRecord {
                entity_id,
                backup,
                local_system,
                lines,
                assignments,
                offset,
                expression_offset,
            });
        }
    }
    records
}

pub(crate) fn reevaluate_expression_records(
    records: &mut [CurveExpressionRecord],
    model_name: Option<&str>,
    external_symbols: &ExternalRelationSymbols,
) {
    for record in records {
        record.assignments =
            evaluate_expression_program(&record.lines, model_name, external_symbols);
    }
}

#[derive(Default)]
pub(crate) struct ExternalRelationSymbols {
    values: BTreeMap<String, Option<CurveExpressionValue>>,
}

impl ExternalRelationSymbols {
    pub(crate) fn observe(&mut self, name: &str, value: Option<CurveExpressionValue>) {
        use std::collections::btree_map::Entry;

        match self.values.entry(expression_identifier_key(name)) {
            Entry::Vacant(entry) => {
                entry.insert(value);
            }
            Entry::Occupied(mut entry) if entry.get() != &value => {
                entry.insert(None);
            }
            Entry::Occupied(_) => {}
        }
    }
}

fn expression_assignment(line: &CurveExpressionLine) -> Option<CurveExpressionAssignment> {
    let source = line.text.trim();
    if source.starts_with("/*") {
        return None;
    }
    let (name, expression) = source.split_once('=')?;
    let name = name.trim();
    let expression = expression.trim();
    let valid_name = !name.is_empty()
        && name.bytes().enumerate().all(|(index, byte)| {
            byte == b'_' || byte.is_ascii_alphabetic() || (index > 0 && byte.is_ascii_digit())
        });
    if !valid_name || expression.is_empty() {
        return None;
    }
    let mut dependencies = Vec::<String>::new();
    let bytes = expression.as_bytes();
    let mut cursor = 0;
    while cursor < bytes.len() {
        if matches!(bytes[cursor], b'\'' | b'"') {
            let delimiter = bytes[cursor];
            cursor += 1;
            while bytes.get(cursor).is_some_and(|byte| *byte != delimiter) {
                cursor += 1;
            }
            if bytes.get(cursor) == Some(&delimiter) {
                cursor += 1;
            }
        } else if bytes[cursor].is_ascii_digit()
            || (bytes[cursor] == b'.' && bytes.get(cursor + 1).is_some_and(u8::is_ascii_digit))
        {
            while bytes
                .get(cursor)
                .is_some_and(|byte| byte.is_ascii_digit() || *byte == b'.')
            {
                cursor += 1;
            }
            if bytes
                .get(cursor)
                .is_some_and(|byte| matches!(byte, b'e' | b'E'))
                && bytes.get(cursor + 1).is_some_and(|byte| {
                    byte.is_ascii_digit()
                        || (matches!(byte, b'+' | b'-')
                            && bytes.get(cursor + 2).is_some_and(u8::is_ascii_digit))
                })
            {
                cursor += 1;
                if bytes
                    .get(cursor)
                    .is_some_and(|byte| matches!(byte, b'+' | b'-'))
                {
                    cursor += 1;
                }
                while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
                    cursor += 1;
                }
            }
        } else if bytes[cursor] == b'_' || bytes[cursor].is_ascii_alphabetic() {
            let start = cursor;
            cursor = expression_identifier_end(bytes, start)?;
            let dependency = &expression[start..cursor];
            let mut following = cursor;
            while bytes.get(following).is_some_and(u8::is_ascii_whitespace) {
                following += 1;
            }
            let function = bytes.get(following) == Some(&b'(')
                && !dependency.contains(':')
                && creo_math_function(dependency).is_some();
            let constant = reserved_relation_scalar(dependency).is_some();
            if !function
                && !constant
                && !dependencies
                    .iter()
                    .any(|existing| existing.eq_ignore_ascii_case(dependency))
            {
                dependencies.push(dependency.to_owned());
            }
        } else {
            cursor += 1;
        }
    }
    Some(CurveExpressionAssignment {
        name: name.to_owned(),
        expression: expression.to_owned(),
        dependencies,
        value: None,
        activation: CurveExpressionActivation::Active,
        offset: line.offset,
    })
}

pub(crate) fn expression_identifier_key(name: &str) -> String {
    name.to_ascii_lowercase()
}

fn reserved_relation_scalar(name: &str) -> Option<f64> {
    if name.eq_ignore_ascii_case("pi") {
        Some(std::f64::consts::PI)
    } else if name.eq_ignore_ascii_case("true") || name.eq_ignore_ascii_case("yes") {
        Some(1.0)
    } else if name.eq_ignore_ascii_case("false") || name.eq_ignore_ascii_case("no") {
        Some(0.0)
    } else {
        None
    }
}

fn expression_identifier_end(source: &[u8], start: usize) -> Option<usize> {
    source
        .get(start)
        .is_some_and(|byte| *byte == b'_' || byte.is_ascii_alphabetic())
        .then_some(())?;
    let mut cursor = start + 1;
    while source
        .get(cursor)
        .is_some_and(|byte| *byte == b'_' || byte.is_ascii_alphabetic() || byte.is_ascii_digit())
    {
        cursor += 1;
    }
    while source.get(cursor) == Some(&b':')
        && source.get(cursor + 1).is_some_and(|byte| {
            *byte == b'_' || byte.is_ascii_alphabetic() || byte.is_ascii_digit()
        })
    {
        cursor += 2;
        while source.get(cursor).is_some_and(|byte| {
            *byte == b'_' || byte.is_ascii_alphabetic() || byte.is_ascii_digit()
        }) {
            cursor += 1;
        }
    }
    Some(cursor)
}

#[derive(Debug, Clone)]
struct ConditionalFrame {
    parent: CurveExpressionActivation,
    condition: Option<bool>,
}

fn conditional_keyword_expression<'a>(source: &'a str, keyword: &str) -> Option<&'a str> {
    let source = source.trim();
    let prefix = source.get(..keyword.len())?;
    prefix.eq_ignore_ascii_case(keyword).then_some(())?;
    source
        .as_bytes()
        .get(keyword.len())
        .is_some_and(u8::is_ascii_whitespace)
        .then_some(())?;
    let expression = source.get(keyword.len()..)?.trim_start();
    (!expression.is_empty()).then_some(expression)
}

fn starts_relation_keyword(source: &str, keyword: &str) -> bool {
    let source = source.trim();
    source
        .get(..keyword.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(keyword))
        && source
            .as_bytes()
            .get(keyword.len())
            .is_none_or(u8::is_ascii_whitespace)
}

fn expression_program_control_is_valid(lines: &[CurveExpressionLine]) -> bool {
    let mut else_seen = Vec::new();
    for line in lines {
        let source = line.text.trim();
        if starts_relation_keyword(source, "if") {
            if conditional_keyword_expression(source, "if").is_none() {
                return false;
            }
            else_seen.push(false);
        } else if starts_relation_keyword(source, "else") {
            if !source.eq_ignore_ascii_case("else") {
                return false;
            }
            let Some(seen) = else_seen.last_mut() else {
                return false;
            };
            if *seen {
                return false;
            }
            *seen = true;
        } else if starts_relation_keyword(source, "endif")
            && (!source.eq_ignore_ascii_case("endif") || else_seen.pop().is_none())
        {
            return false;
        }
    }
    else_seen.is_empty()
}

fn branch_activation(
    parent: CurveExpressionActivation,
    condition: Option<bool>,
    alternative: bool,
) -> CurveExpressionActivation {
    match parent {
        CurveExpressionActivation::Inactive => CurveExpressionActivation::Inactive,
        CurveExpressionActivation::Conditional => CurveExpressionActivation::Conditional,
        CurveExpressionActivation::Active => match condition {
            Some(selected) if selected != alternative => CurveExpressionActivation::Active,
            Some(_) => CurveExpressionActivation::Inactive,
            None => CurveExpressionActivation::Conditional,
        },
    }
}

fn evaluate_expression_program(
    lines: &[CurveExpressionLine],
    model_name: Option<&str>,
    external_symbols: &ExternalRelationSymbols,
) -> Vec<CurveExpressionAssignment> {
    if !expression_program_control_is_valid(lines) {
        return lines
            .iter()
            .filter_map(expression_assignment)
            .map(|mut assignment| {
                assignment.activation = CurveExpressionActivation::Conditional;
                assignment
            })
            .collect();
    }

    let mut existing_symbols = external_symbols
        .values
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    existing_symbols.extend(
        lines
            .iter()
            .filter_map(expression_assignment)
            .map(|assignment| expression_identifier_key(&assignment.name)),
    );
    let context = RelationEvaluationContext {
        model_name,
        existing_symbols: Some(&existing_symbols),
    };
    let mut values = external_symbols
        .values
        .iter()
        .filter_map(|(name, value)| value.clone().map(|value| (name.clone(), value)))
        .collect::<BTreeMap<_, _>>();
    let mut stack = Vec::<ConditionalFrame>::new();
    let mut activity = CurveExpressionActivation::Active;
    let mut assignments = Vec::new();
    for line in lines {
        let source = line.text.trim();
        if let Some(condition_source) = conditional_keyword_expression(source, "if") {
            let condition = (activity == CurveExpressionActivation::Active)
                .then(|| evaluate_relation_expression(condition_source, &values, context))
                .flatten()
                .and_then(|value| value.truth());
            let parent = activity;
            activity = branch_activation(parent, condition, false);
            stack.push(ConditionalFrame { parent, condition });
            continue;
        }
        if source.eq_ignore_ascii_case("else") {
            let frame = stack.last().expect("validated conditional stack");
            activity = branch_activation(frame.parent, frame.condition, true);
            continue;
        }
        if source.eq_ignore_ascii_case("endif") {
            let frame = stack.pop().expect("validated conditional stack");
            activity = frame.parent;
            continue;
        }
        let Some(mut assignment) = expression_assignment(line) else {
            continue;
        };
        assignment.activation = activity;
        let key = expression_identifier_key(&assignment.name);
        match activity {
            CurveExpressionActivation::Active => {
                assignment.value =
                    evaluate_relation_expression(&assignment.expression, &values, context);
                if let Some(value) = assignment.value.clone() {
                    values.insert(key, value);
                } else {
                    values.remove(&key);
                }
            }
            CurveExpressionActivation::Inactive => {}
            CurveExpressionActivation::Conditional => {
                values.remove(&key);
            }
        }
        assignments.push(assignment);
    }
    assignments
}

#[derive(Clone, Copy, Default)]
struct RelationEvaluationContext<'a> {
    model_name: Option<&'a str>,
    existing_symbols: Option<&'a BTreeSet<String>>,
}

trait ExpressionValue: Clone {
    fn number(value: f64) -> Self;
    fn string(_value: String) -> Option<Self> {
        None
    }
    fn add(self, right: Self) -> Option<Self>;
    fn subtract(self, right: Self) -> Option<Self>;
    fn multiply(self, right: Self) -> Option<Self>;
    fn divide(self, right: Self) -> Option<Self>;
    fn power(self, right: Self) -> Option<Self>;
    fn compare(self, right: Self, operator: ComparisonOperator) -> Option<Self>;
    fn logical_and(self, right: Self) -> Option<Self>;
    fn logical_or(self, right: Self) -> Option<Self>;
    fn logical_not(self) -> Option<Self>;
    fn function(
        name: CreoMathFunction,
        arguments: &[Self],
        context: RelationEvaluationContext<'_>,
    ) -> Option<Self>;
    fn negate(self) -> Option<Self>;
    fn finite(self) -> bool;
}

impl ExpressionValue for f64 {
    fn number(value: f64) -> Self {
        value
    }

    fn add(self, right: Self) -> Option<Self> {
        Some(self + right)
    }

    fn subtract(self, right: Self) -> Option<Self> {
        Some(self - right)
    }

    fn multiply(self, right: Self) -> Option<Self> {
        Some(self * right)
    }

    fn divide(self, right: Self) -> Option<Self> {
        Some(self / right)
    }

    fn power(self, right: Self) -> Option<Self> {
        Some(self.powf(right))
    }

    fn compare(self, right: Self, operator: ComparisonOperator) -> Option<Self> {
        Some(f64::from(operator.evaluate(self, right)))
    }

    fn logical_and(self, right: Self) -> Option<Self> {
        Some(f64::from(self != 0.0 && right != 0.0))
    }

    fn logical_or(self, right: Self) -> Option<Self> {
        Some(f64::from(self != 0.0 || right != 0.0))
    }

    fn logical_not(self) -> Option<Self> {
        Some(f64::from(self == 0.0))
    }

    fn function(
        name: CreoMathFunction,
        arguments: &[Self],
        _context: RelationEvaluationContext<'_>,
    ) -> Option<Self> {
        evaluate_creo_math_function(name, arguments)
    }

    fn negate(self) -> Option<Self> {
        Some(-self)
    }

    fn finite(self) -> bool {
        self.is_finite()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct AffineValue {
    constant: f64,
    linear: f64,
}

impl ExpressionValue for AffineValue {
    fn number(value: f64) -> Self {
        Self {
            constant: value,
            linear: 0.0,
        }
    }

    fn add(self, right: Self) -> Option<Self> {
        Some(Self {
            constant: self.constant + right.constant,
            linear: self.linear + right.linear,
        })
    }

    fn subtract(self, right: Self) -> Option<Self> {
        Some(Self {
            constant: self.constant - right.constant,
            linear: self.linear - right.linear,
        })
    }

    fn multiply(self, right: Self) -> Option<Self> {
        (self.linear == 0.0 || right.linear == 0.0).then_some(Self {
            constant: self.constant * right.constant,
            linear: self.constant * right.linear + self.linear * right.constant,
        })
    }

    fn divide(self, right: Self) -> Option<Self> {
        (right.linear == 0.0 && right.constant != 0.0).then_some(Self {
            constant: self.constant / right.constant,
            linear: self.linear / right.constant,
        })
    }

    fn power(self, right: Self) -> Option<Self> {
        if right.linear == 0.0 && right.constant == 1.0 {
            return Some(self);
        }
        if right.linear == 0.0 && right.constant == 0.0 {
            return Some(Self::number(1.0));
        }
        (self.linear == 0.0 && right.linear == 0.0)
            .then(|| self.constant.powf(right.constant))
            .filter(|value| value.is_finite())
            .map(Self::number)
    }

    fn compare(self, right: Self, operator: ComparisonOperator) -> Option<Self> {
        (self.linear == 0.0 && right.linear == 0.0)
            .then(|| Self::number(f64::from(operator.evaluate(self.constant, right.constant))))
    }

    fn logical_and(self, right: Self) -> Option<Self> {
        (self.linear == 0.0 && right.linear == 0.0)
            .then(|| Self::number(f64::from(self.constant != 0.0 && right.constant != 0.0)))
    }

    fn logical_or(self, right: Self) -> Option<Self> {
        (self.linear == 0.0 && right.linear == 0.0)
            .then(|| Self::number(f64::from(self.constant != 0.0 || right.constant != 0.0)))
    }

    fn logical_not(self) -> Option<Self> {
        (self.linear == 0.0).then(|| Self::number(f64::from(self.constant == 0.0)))
    }

    fn function(
        name: CreoMathFunction,
        arguments: &[Self],
        _context: RelationEvaluationContext<'_>,
    ) -> Option<Self> {
        let constants = arguments
            .iter()
            .map(|argument| (argument.linear == 0.0).then_some(argument.constant))
            .collect::<Option<Vec<_>>>()?;
        evaluate_creo_math_function(name, &constants).map(Self::number)
    }

    fn negate(self) -> Option<Self> {
        Some(Self {
            constant: -self.constant,
            linear: -self.linear,
        })
    }

    fn finite(self) -> bool {
        self.constant.is_finite() && self.linear.is_finite()
    }
}

impl ExpressionValue for CurveExpressionValue {
    fn number(value: f64) -> Self {
        Self::Number(value)
    }

    fn string(value: String) -> Option<Self> {
        Some(Self::String(value))
    }

    fn add(self, right: Self) -> Option<Self> {
        match (self, right) {
            (Self::Number(left), Self::Number(right)) => Some(Self::Number(left + right)),
            (Self::String(mut left), Self::String(right)) => {
                left.push_str(&right);
                Some(Self::String(left))
            }
            _ => None,
        }
    }

    fn subtract(self, right: Self) -> Option<Self> {
        numeric_binary(self, right, |left, right| left - right)
    }

    fn multiply(self, right: Self) -> Option<Self> {
        numeric_binary(self, right, |left, right| left * right)
    }

    fn divide(self, right: Self) -> Option<Self> {
        numeric_binary(self, right, |left, right| left / right)
    }

    fn power(self, right: Self) -> Option<Self> {
        numeric_binary(self, right, f64::powf)
    }

    fn compare(self, right: Self, operator: ComparisonOperator) -> Option<Self> {
        let result = match (self, right) {
            (Self::Number(left), Self::Number(right)) => operator.evaluate(left, right),
            (Self::String(left), Self::String(right)) => match operator {
                ComparisonOperator::Equal => left == right,
                ComparisonOperator::NotEqual => left != right,
                _ => return None,
            },
            _ => return None,
        };
        Some(Self::Number(f64::from(result)))
    }

    fn logical_and(self, right: Self) -> Option<Self> {
        numeric_binary(self, right, |left, right| {
            f64::from(left != 0.0 && right != 0.0)
        })
    }

    fn logical_or(self, right: Self) -> Option<Self> {
        numeric_binary(self, right, |left, right| {
            f64::from(left != 0.0 || right != 0.0)
        })
    }

    fn logical_not(self) -> Option<Self> {
        let Self::Number(value) = self else {
            return None;
        };
        Some(Self::Number(f64::from(value == 0.0)))
    }

    fn function(
        name: CreoMathFunction,
        arguments: &[Self],
        context: RelationEvaluationContext<'_>,
    ) -> Option<Self> {
        evaluate_creo_relation_function(name, arguments, context)
    }

    fn negate(self) -> Option<Self> {
        match self {
            Self::Number(value) => Some(Self::Number(-value)),
            Self::String(_) => None,
        }
    }

    fn finite(self) -> bool {
        match self {
            Self::Number(value) => value.is_finite(),
            Self::String(_) => true,
        }
    }
}

fn numeric_binary(
    left: CurveExpressionValue,
    right: CurveExpressionValue,
    operation: impl FnOnce(f64, f64) -> f64,
) -> Option<CurveExpressionValue> {
    match (left, right) {
        (CurveExpressionValue::Number(left), CurveExpressionValue::Number(right)) => {
            Some(CurveExpressionValue::Number(operation(left, right)))
        }
        _ => None,
    }
}

struct ExpressionParser<'a, V> {
    source: &'a [u8],
    cursor: usize,
    values: &'a BTreeMap<String, V>,
    context: RelationEvaluationContext<'a>,
    nesting: usize,
}

const MAX_EXPRESSION_NESTING: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ComparisonOperator {
    Equal,
    NotEqual,
    Greater,
    GreaterOrEqual,
    Less,
    LessOrEqual,
}

impl ComparisonOperator {
    fn evaluate(self, left: f64, right: f64) -> bool {
        match self {
            Self::Equal => left == right,
            Self::NotEqual => left != right,
            Self::Greater => left > right,
            Self::GreaterOrEqual => left >= right,
            Self::Less => left < right,
            Self::LessOrEqual => left <= right,
        }
    }
}

impl<V: ExpressionValue> ExpressionParser<'_, V> {
    fn whitespace(&mut self) {
        while self
            .source
            .get(self.cursor)
            .is_some_and(u8::is_ascii_whitespace)
        {
            self.cursor += 1;
        }
    }

    fn logical_or(&mut self) -> Option<V> {
        let mut value = self.logical_and()?;
        loop {
            self.whitespace();
            if self.source.get(self.cursor) != Some(&b'|') {
                return Some(value);
            }
            self.cursor += 1;
            value = value.logical_or(self.logical_and()?)?;
        }
    }

    fn logical_and(&mut self) -> Option<V> {
        let mut value = self.comparison()?;
        loop {
            self.whitespace();
            if self.source.get(self.cursor) != Some(&b'&') {
                return Some(value);
            }
            self.cursor += 1;
            value = value.logical_and(self.comparison()?)?;
        }
    }

    fn comparison(&mut self) -> Option<V> {
        let value = self.expression()?;
        self.whitespace();
        let (operator, width) = match self.source.get(self.cursor..) {
            Some([b'=', b'=', ..]) => (ComparisonOperator::Equal, 2),
            Some([b'!' | b'~', b'=', ..] | [b'<', b'>', ..]) => (ComparisonOperator::NotEqual, 2),
            Some([b'>', b'=', ..]) => (ComparisonOperator::GreaterOrEqual, 2),
            Some([b'<', b'=', ..]) => (ComparisonOperator::LessOrEqual, 2),
            Some([b'>', ..]) => (ComparisonOperator::Greater, 1),
            Some([b'<', ..]) => (ComparisonOperator::Less, 1),
            _ => return Some(value),
        };
        self.cursor += width;
        value.compare(self.expression()?, operator)
    }

    fn expression(&mut self) -> Option<V> {
        let mut value = self.term()?;
        loop {
            self.whitespace();
            match self.source.get(self.cursor) {
                Some(b'+') => {
                    self.cursor += 1;
                    value = value.add(self.term()?)?;
                }
                Some(b'-') => {
                    self.cursor += 1;
                    value = value.subtract(self.term()?)?;
                }
                _ => return Some(value),
            }
        }
    }

    fn term(&mut self) -> Option<V> {
        let mut value = self.unary()?;
        loop {
            self.whitespace();
            match self.source.get(self.cursor) {
                Some(b'*') => {
                    self.cursor += 1;
                    value = value.multiply(self.unary()?)?;
                }
                Some(b'/') => {
                    self.cursor += 1;
                    value = value.divide(self.unary()?)?;
                }
                _ => return Some(value),
            }
        }
    }

    fn unary(&mut self) -> Option<V> {
        self.whitespace();
        let mut operators = Vec::new();
        loop {
            match self.source.get(self.cursor) {
                Some(b'+') => self.cursor += 1,
                Some(b'-') => {
                    operators.push(b'-');
                    self.cursor += 1;
                }
                Some(b'!' | b'~') => {
                    operators.push(b'!');
                    self.cursor += 1;
                }
                _ => break,
            }
            self.whitespace();
        }
        let mut value = self.power()?;
        for operator in operators.into_iter().rev() {
            value = if operator == b'-' {
                value.negate()?
            } else {
                value.logical_not()?
            };
        }
        Some(value)
    }

    fn power(&mut self) -> Option<V> {
        let value = self.primary()?;
        self.whitespace();
        if self.source.get(self.cursor) != Some(&b'^') {
            return Some(value);
        }
        (self.nesting < MAX_EXPRESSION_NESTING).then_some(())?;
        self.cursor += 1;
        self.nesting += 1;
        let exponent = self.unary()?;
        self.nesting -= 1;
        value.power(exponent)
    }

    fn primary(&mut self) -> Option<V> {
        self.whitespace();
        let value = match self.source.get(self.cursor)? {
            b'(' => {
                (self.nesting < MAX_EXPRESSION_NESTING).then_some(())?;
                self.cursor += 1;
                self.nesting += 1;
                let value = self.logical_or()?;
                self.nesting -= 1;
                self.whitespace();
                (self.source.get(self.cursor) == Some(&b')')).then(|| {
                    self.cursor += 1;
                    value
                })
            }
            byte if byte.is_ascii_digit() || *byte == b'.' => self.number(),
            b'\'' | b'"' => self.string(),
            byte if byte.is_ascii_alphabetic() || *byte == b'_' => self.identifier_or_function(),
            _ => None,
        }?;
        Some(value)
    }

    fn string(&mut self) -> Option<V> {
        let delimiter = *self.source.get(self.cursor)?;
        self.cursor += 1;
        let start = self.cursor;
        while self
            .source
            .get(self.cursor)
            .is_some_and(|byte| *byte != delimiter)
        {
            self.cursor += 1;
        }
        (self.source.get(self.cursor) == Some(&delimiter)).then_some(())?;
        let value = std::str::from_utf8(&self.source[start..self.cursor])
            .ok()?
            .to_owned();
        self.cursor += 1;
        V::string(value)
    }

    fn number(&mut self) -> Option<V> {
        let start = self.cursor;
        while self
            .source
            .get(self.cursor)
            .is_some_and(|byte| byte.is_ascii_digit() || *byte == b'.')
        {
            self.cursor += 1;
        }
        if self
            .source
            .get(self.cursor)
            .is_some_and(|byte| matches!(byte, b'e' | b'E'))
        {
            self.cursor += 1;
            if self
                .source
                .get(self.cursor)
                .is_some_and(|byte| matches!(byte, b'+' | b'-'))
            {
                self.cursor += 1;
            }
            while self.source.get(self.cursor).is_some_and(u8::is_ascii_digit) {
                self.cursor += 1;
            }
        }
        let value = std::str::from_utf8(&self.source[start..self.cursor])
            .ok()?
            .parse()
            .ok()?;
        Some(V::number(value))
    }

    fn identifier_or_function(&mut self) -> Option<V> {
        let start = self.cursor;
        self.cursor = expression_identifier_end(self.source, start)?;
        let name = std::str::from_utf8(&self.source[start..self.cursor]).ok()?;
        self.whitespace();
        if self.source.get(self.cursor) != Some(&b'(') {
            if let Some(value) = reserved_relation_scalar(name) {
                return Some(V::number(value));
            }
            return self.values.get(&expression_identifier_key(name)).cloned();
        }
        (!name.contains(':')).then_some(())?;
        (self.nesting < MAX_EXPRESSION_NESTING).then_some(())?;
        let function = creo_math_function(name)?;
        self.cursor += 1;
        self.nesting += 1;
        self.whitespace();
        let mut arguments = Vec::new();
        if self.source.get(self.cursor) != Some(&b')') {
            loop {
                arguments.push(self.logical_or()?);
                self.whitespace();
                if self.source.get(self.cursor) != Some(&b',') {
                    break;
                }
                self.cursor += 1;
            }
        }
        self.whitespace();
        (self.source.get(self.cursor) == Some(&b')')).then_some(())?;
        self.cursor += 1;
        self.nesting -= 1;
        V::function(function, &arguments, self.context)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CreoMathFunction {
    Sin,
    Cos,
    Tan,
    Asin,
    Acos,
    Atan,
    Atan2,
    Sinh,
    Cosh,
    Tanh,
    Sign,
    Mod,
    If,
    Bound,
    Dead,
    Near,
    Min,
    Max,
    Log,
    Ln,
    Exp,
    Pow,
    Sqrt,
    Abs,
    Ceil,
    Floor,
    DblInTol,
    Itos,
    Rtos,
    RelModelName,
    RelModelType,
    Exists,
    Search,
    Extract,
    StringLength,
    StringStarts,
    StringEnds,
    StringMatch,
    StringPattern,
}

fn creo_math_function(name: &str) -> Option<CreoMathFunction> {
    match name.to_ascii_lowercase().as_str() {
        "sin" => Some(CreoMathFunction::Sin),
        "cos" => Some(CreoMathFunction::Cos),
        "tan" => Some(CreoMathFunction::Tan),
        "asin" => Some(CreoMathFunction::Asin),
        "acos" => Some(CreoMathFunction::Acos),
        "atan" => Some(CreoMathFunction::Atan),
        "atan2" => Some(CreoMathFunction::Atan2),
        "sinh" => Some(CreoMathFunction::Sinh),
        "cosh" => Some(CreoMathFunction::Cosh),
        "tanh" => Some(CreoMathFunction::Tanh),
        "sign" => Some(CreoMathFunction::Sign),
        "mod" => Some(CreoMathFunction::Mod),
        "if" => Some(CreoMathFunction::If),
        "bound" => Some(CreoMathFunction::Bound),
        "dead" => Some(CreoMathFunction::Dead),
        "near" => Some(CreoMathFunction::Near),
        "min" => Some(CreoMathFunction::Min),
        "max" => Some(CreoMathFunction::Max),
        "log" => Some(CreoMathFunction::Log),
        "ln" => Some(CreoMathFunction::Ln),
        "exp" => Some(CreoMathFunction::Exp),
        "pow" => Some(CreoMathFunction::Pow),
        "sqrt" => Some(CreoMathFunction::Sqrt),
        "abs" => Some(CreoMathFunction::Abs),
        "ceil" => Some(CreoMathFunction::Ceil),
        "floor" => Some(CreoMathFunction::Floor),
        "dbl_in_tol" => Some(CreoMathFunction::DblInTol),
        "itos" => Some(CreoMathFunction::Itos),
        "rtos" => Some(CreoMathFunction::Rtos),
        "rel_model_name" => Some(CreoMathFunction::RelModelName),
        "rel_model_type" => Some(CreoMathFunction::RelModelType),
        "exists" => Some(CreoMathFunction::Exists),
        "search" => Some(CreoMathFunction::Search),
        "extract" => Some(CreoMathFunction::Extract),
        "string_length" => Some(CreoMathFunction::StringLength),
        "string_starts" => Some(CreoMathFunction::StringStarts),
        "string_ends" => Some(CreoMathFunction::StringEnds),
        "string_match" => Some(CreoMathFunction::StringMatch),
        "string_pattern" => Some(CreoMathFunction::StringPattern),
        _ => None,
    }
}

fn evaluate_creo_math_function(name: CreoMathFunction, arguments: &[f64]) -> Option<f64> {
    let value = match (name, arguments) {
        (CreoMathFunction::Sin, [x]) => x.to_radians().sin(),
        (CreoMathFunction::Cos, [x]) => x.to_radians().cos(),
        (CreoMathFunction::Tan, [x]) => x.to_radians().tan(),
        (CreoMathFunction::Asin, [x]) => x.asin().to_degrees(),
        (CreoMathFunction::Acos, [x]) => x.acos().to_degrees(),
        (CreoMathFunction::Atan, [x]) => x.atan().to_degrees(),
        (CreoMathFunction::Atan2, [y, x]) => y.atan2(*x).to_degrees(),
        (CreoMathFunction::Sinh, [x]) if x.abs() <= 85.0 => x.sinh(),
        (CreoMathFunction::Cosh, [x]) if x.abs() <= 85.0 => x.cosh(),
        (CreoMathFunction::Tanh, [x]) if x.abs() <= 85.0 => x.tanh(),
        (CreoMathFunction::Sign, [x, y]) => {
            if *y < 0.0 {
                -x.abs()
            } else {
                x.abs()
            }
        }
        (CreoMathFunction::Mod, [x, y]) if *y != 0.0 => x - (x / y).trunc() * y,
        (CreoMathFunction::If, [condition, when_true, when_false]) => {
            if *condition == 0.0 {
                *when_false
            } else {
                *when_true
            }
        }
        (CreoMathFunction::Bound, [x, lower, upper]) if lower < upper => x.clamp(*lower, *upper),
        (CreoMathFunction::Dead, [x, lower, upper]) if lower <= upper => {
            if x < lower {
                x - lower
            } else if x > upper {
                x - upper
            } else {
                0.0
            }
        }
        (CreoMathFunction::Near, [x, y, delta]) if *delta >= 0.0 => {
            ((x - y).abs() <= *delta) as u8 as f64
        }
        (CreoMathFunction::Min, [x, y]) => x.min(*y),
        (CreoMathFunction::Max, [x, y]) => x.max(*y),
        (CreoMathFunction::Log, [x]) => x.log10(),
        (CreoMathFunction::Ln, [x]) => x.ln(),
        (CreoMathFunction::Exp, [x]) => x.exp(),
        (CreoMathFunction::Pow, [base, exponent]) => base.powf(*exponent),
        (CreoMathFunction::Sqrt, [x]) => x.sqrt(),
        (CreoMathFunction::Abs, [x]) => x.abs(),
        (CreoMathFunction::Ceil, [x]) => (x - 1e-9).ceil(),
        (CreoMathFunction::Floor, [x]) => (x + 1e-9).floor(),
        (CreoMathFunction::DblInTol, [first, second, tolerance]) if *tolerance >= 0.0 => {
            ((first - second).abs() <= *tolerance) as u8 as f64
        }
        _ => return None,
    };
    value.is_finite().then_some(value)
}

fn evaluate_creo_relation_function(
    name: CreoMathFunction,
    arguments: &[CurveExpressionValue],
    context: RelationEvaluationContext<'_>,
) -> Option<CurveExpressionValue> {
    use CurveExpressionValue::{Number, String};
    let value = match (name, arguments) {
        (CreoMathFunction::Itos, [Number(value)]) if value.is_finite() => {
            let rounded = value.round();
            if rounded == 0.0 {
                String(std::string::String::new())
            } else if rounded >= i64::MIN as f64 && rounded <= i64::MAX as f64 {
                String(format!("{rounded:.0}"))
            } else {
                return None;
            }
        }
        (CreoMathFunction::Rtos, [Number(value)]) => {
            String(format_relation_real(*value, None, false)?)
        }
        (CreoMathFunction::Rtos, [Number(value), Number(decimals)]) => String(
            format_relation_real(*value, Some(relation_precision(*decimals)?), false)?,
        ),
        (CreoMathFunction::Rtos, [Number(value), Number(decimals), Number(scientific)]) => {
            String(format_relation_real(
                *value,
                Some(relation_precision(*decimals)?),
                *scientific != 0.0,
            )?)
        }
        (CreoMathFunction::RelModelName, []) => String(context.model_name?.to_owned()),
        (CreoMathFunction::RelModelType, []) => String("part".to_owned()),
        (CreoMathFunction::Exists, [String(name)])
            if context
                .existing_symbols?
                .contains(&expression_identifier_key(name)) =>
        {
            Number(1.0)
        }
        (CreoMathFunction::Search, [String(value), String(needle)]) => {
            let position = value
                .find(needle)
                .map_or(0, |byte| value[..byte].chars().count() + 1);
            Number(position as f64)
        }
        (CreoMathFunction::Extract, [String(value), Number(position), Number(length)]) => {
            let (position, length) = integer_pair(*position, *length)?;
            if position == 0 {
                return None;
            }
            String(value.chars().skip(position - 1).take(length).collect())
        }
        (CreoMathFunction::StringLength, [String(value)]) => Number(value.chars().count() as f64),
        (CreoMathFunction::StringStarts, [String(value), String(prefix)]) => {
            Number(f64::from(value.starts_with(prefix)))
        }
        (CreoMathFunction::StringEnds, [String(value), String(suffix)]) => {
            Number(f64::from(value.ends_with(suffix)))
        }
        (CreoMathFunction::StringMatch, [String(value), String(expected)]) => {
            Number(f64::from(value == expected))
        }
        (CreoMathFunction::StringPattern, [String(value), String(pattern)]) => {
            Number(f64::from(relation_string_pattern(value, pattern)?))
        }
        _ => {
            let numbers = arguments
                .iter()
                .map(|argument| match argument {
                    Number(value) => Some(*value),
                    String(_) => None,
                })
                .collect::<Option<Vec<_>>>()?;
            Number(evaluate_creo_math_function(name, &numbers)?)
        }
    };
    value.clone().finite().then_some(value)
}

fn relation_string_pattern(value: &str, pattern: &str) -> Option<bool> {
    regex::RegexBuilder::new(&format!(r"\A(?:{pattern})\z"))
        .size_limit(1 << 20)
        .dfa_size_limit(1 << 20)
        .build()
        .ok()
        .map(|pattern| pattern.is_match(value))
}

const MAX_RELATION_STRING_PRECISION: usize = 128;

fn relation_precision(value: f64) -> Option<usize> {
    (value.is_finite()
        && value.fract() == 0.0
        && value >= 0.0
        && value <= MAX_RELATION_STRING_PRECISION as f64)
        .then_some(value as usize)
}

fn format_relation_real(value: f64, decimals: Option<usize>, scientific: bool) -> Option<String> {
    if !value.is_finite() {
        return None;
    }
    if value == 0.0 {
        return Some(String::new());
    }
    let Some(decimals) = decimals else {
        return Some(value.to_string());
    };
    if !scientific {
        return Some(format!("{value:.decimals$}"));
    }
    let formatted = format!("{value:.decimals$e}");
    let (mantissa, exponent) = formatted.split_once('e')?;
    let exponent = exponent.parse::<i32>().ok()?;
    Some(format!(
        "{mantissa}e{}{magnitude:02}",
        if exponent < 0 { "-" } else { "" },
        magnitude = exponent.unsigned_abs()
    ))
}

fn integer_pair(first: f64, second: f64) -> Option<(usize, usize)> {
    (first.is_finite()
        && second.is_finite()
        && first.fract() == 0.0
        && second.fract() == 0.0
        && first >= 0.0
        && second >= 0.0
        && first <= usize::MAX as f64
        && second <= usize::MAX as f64)
        .then_some((first as usize, second as usize))
}

fn evaluate_relation_expression(
    expression: &str,
    values: &BTreeMap<String, CurveExpressionValue>,
    context: RelationEvaluationContext<'_>,
) -> Option<CurveExpressionValue> {
    let mut parser = ExpressionParser {
        source: expression.as_bytes(),
        cursor: 0,
        values,
        context,
        nesting: 0,
    };
    let value = parser.logical_or()?;
    parser.whitespace();
    (parser.cursor == parser.source.len() && value.clone().finite()).then_some(value)
}

#[cfg(test)]
fn evaluate_expression(expression: &str, values: &BTreeMap<String, f64>) -> Option<f64> {
    let mut parser = ExpressionParser {
        source: expression.as_bytes(),
        cursor: 0,
        values,
        context: RelationEvaluationContext::default(),
        nesting: 0,
    };
    let value = parser.logical_or()?;
    parser.whitespace();
    (parser.cursor == parser.source.len() && value.finite()).then_some(value)
}

fn evaluate_affine_expression(
    expression: &str,
    values: &BTreeMap<String, AffineValue>,
) -> Option<AffineValue> {
    let mut parser = ExpressionParser {
        source: expression.as_bytes(),
        cursor: 0,
        values,
        context: RelationEvaluationContext::default(),
        nesting: 0,
    };
    let value = parser.logical_or()?;
    parser.whitespace();
    (parser.cursor == parser.source.len() && value.finite()).then_some(value)
}

fn evaluate_affine_program(record: &CurveExpressionRecord) -> BTreeMap<String, AffineValue> {
    let mut values = BTreeMap::from([(
        "t".to_string(),
        AffineValue {
            constant: 0.0,
            linear: 1.0,
        },
    )]);
    for assignment in &record.assignments {
        let key = expression_identifier_key(&assignment.name);
        match assignment.activation {
            CurveExpressionActivation::Active => {
                if let Some(value) = evaluate_affine_expression(&assignment.expression, &values) {
                    values.insert(key, value);
                } else {
                    values.remove(&key);
                }
            }
            CurveExpressionActivation::Inactive => {}
            CurveExpressionActivation::Conditional => {
                values.remove(&key);
            }
        }
    }
    values
}

/// Recognize an exact cylindrical helix program expressed by the conventional
/// Creo outputs `r`, `theta` (degrees), and `z` over `t` in `[0, 1]`.
pub fn expression_helix(record: &CurveExpressionRecord) -> Option<CurveExpressionHelix> {
    let values = evaluate_affine_program(record);
    let radius = values.get("r")?;
    let theta = values.get("theta")?;
    let z = values.get("z")?;
    if radius.constant <= 0.0 || radius.linear != 0.0 {
        return None;
    }
    let angular_travel = theta.linear;
    let revolutions = angular_travel.abs() / 360.0;
    (revolutions > 0.0).then_some(CurveExpressionHelix {
        radius: radius.constant,
        height: z.linear,
        z_start: z.constant,
        revolutions,
        start_angle: theta.constant.to_radians(),
        clockwise: angular_travel < 0.0,
    })
}

/// Decode positional `crv_array` rows whose terminal
/// `<four canonical reference IDs> 00 00 e3 e1 e3` suffix has exactly one
/// possible boundary. Rows with ambiguous or malformed suffixes are not
/// returned; callers must preserve their enclosing section as unknown data.
pub fn topology_rows(payload: &[u8]) -> Vec<CurveTopologyRow> {
    let mut rows = framed_rows(payload)
        .into_iter()
        .filter_map(|row| parse_topology_row(&payload[row.start..row.end], row.start))
        .collect::<Vec<_>>();
    rows.sort_by_key(|row| row.offset);
    rows.dedup_by_key(|row| row.offset);
    rows
}

/// Decode a complete DEPDB `crv_array\0 f2 f8 <count>` cross-section array.
/// Any malformed row or count mismatch withholds the entire array.
#[must_use]
pub fn depdb_cross_section_rows(payload: &[u8]) -> Vec<DepdbCurveRow> {
    let Some(array) = find(payload, b"crv_array\0", 0) else {
        return Vec::new();
    };
    let header = array + b"crv_array\0".len();
    if payload.get(header..header + 2) != Some(&[0xf2, psb::token::ARRAY_OPEN]) {
        return Vec::new();
    }
    let (count, after_count) = compact_int(payload, header + 2);
    if after_count == header + 2 {
        return Vec::new();
    }
    let Ok(count) = usize::try_from(count) else {
        return Vec::new();
    };
    if count == 0 || prototypes(payload).len() != 1 {
        return Vec::new();
    }
    let Some(topology) = find(payload, b"topol_ref_data\0", after_count) else {
        return Vec::new();
    };
    let mut cursor = topology + b"topol_ref_data\0".len();
    let cache = scalar::ScalarCache::from_section(payload);
    let positional_count = count - 1;
    // Each row consumes at least one payload byte past the topology cursor
    // before its terminator, so the row count cannot exceed the unread bytes.
    let capacity = bounded_len(
        positional_count as u64,
        1,
        payload.len().saturating_sub(cursor),
    )
    .unwrap_or(0);
    let mut rows = Vec::with_capacity(capacity);
    let mut boundaries = Vec::new();
    for (marker, length) in [
        (b"\xe1\xe3".as_slice(), 2),
        (b"\xe1\xf5\x05\xf6\xe3", 5),
        (b"\xe1\xe0", 1),
    ] {
        let mut search = cursor;
        while let Some(offset) = find(payload, marker, search) {
            boundaries.push((offset, length));
            search = offset + marker.len();
        }
    }
    boundaries.sort_unstable();
    boundaries.dedup();
    while rows.len() < positional_count {
        let first_candidate = boundaries.partition_point(|(end, _)| *end < cursor);
        let Some((row, terminator, length)) = boundaries[first_candidate..]
            .iter()
            .copied()
            .find_map(|(end, length)| {
                let row = parse_depdb_curve_segment(&payload[cursor..end], cursor, &cache)?;
                Some((row, end, length))
            })
        else {
            return Vec::new();
        };
        rows.push(row);
        cursor = terminator + length;
    }
    if rows.len() == positional_count {
        rows
    } else {
        Vec::new()
    }
}

fn parse_depdb_curve_segment(
    segment: &[u8],
    absolute_offset: usize,
    cache: &scalar::ScalarCache,
) -> Option<DepdbCurveRow> {
    let suffixes = (4..=11)
        .filter_map(|suffix_length| {
            let start = segment.len().checked_sub(suffix_length)?;
            let (zero0, p1) = compact_int(segment, start);
            let (x1, p2) = compact_int(segment, p1);
            let (f1, p3) = compact_int(segment, p2);
            let (zero1, end) = compact_int(segment, p3);
            (p1 > start && p2 > p1 && p3 > p2 && end == segment.len())
                .then_some((start, [zero0, x1, f1, zero1]))
        })
        .filter(|(_, suffix)| suffix[0] == 0 && suffix[3] == 0)
        .collect::<Vec<_>>();
    let [(suffix_start, suffix)] = suffixes.as_slice() else {
        return None;
    };
    let prefixes = (0..*suffix_start).filter_map(|start| {
        let prefix = topology_prefix_fields(segment, start)?;
        (prefix.end <= *suffix_start).then_some((start, prefix))
    });
    let prefixes = prefixes
        .fold(BTreeMap::new(), |mut by_end, (start, prefix)| {
            by_end
                .entry(prefix.end)
                .and_modify(|(known_start, known_prefix)| {
                    if start < *known_start {
                        *known_start = start;
                        *known_prefix = prefix;
                    }
                })
                .or_insert((start, prefix));
            by_end
        })
        .into_values()
        .collect::<Vec<_>>();
    let [(row_start, prefix)] = prefixes.as_slice() else {
        return None;
    };
    let body = segment[prefix.end..*suffix_start].to_vec();
    let (scalar_tokens, references, opaque_spans) =
        curve_scalar_lane(&body, prefix.type_byte, cache);
    Some(DepdbCurveRow {
        id: prefix.id,
        type_byte: prefix.type_byte,
        feature_id: prefix.feature_id,
        directions: prefix.directions,
        suffix: *suffix,
        body,
        scalar_tokens,
        references,
        opaque_spans,
        offset: absolute_offset + row_start,
    })
}

#[derive(Debug, Clone, Copy)]
struct FramedRow {
    start: usize,
    end: usize,
}

#[derive(Debug, Clone, Copy)]
struct TopologyPrefix {
    id: u32,
    type_byte: u8,
    feature_id: u32,
    directions: [u8; 2],
    end: usize,
}

fn row_terminator(payload: &[u8], start: usize, end: usize) -> Option<(usize, usize)> {
    let short = find_in(payload, b"\xe1\xe3", start, end).map(|offset| (offset, 2));
    let long_search_end = short.map_or(end, |(offset, _)| {
        offset
            .saturating_add(b"\xe1\xf5\x05\xf6\xe3".len())
            .min(end)
    });
    let long =
        find_in(payload, b"\xe1\xf5\x05\xf6\xe3", start, long_search_end).map(|offset| (offset, 5));
    match (short, long) {
        (Some(left), Some(right)) => Some(if left.0 < right.0 { left } else { right }),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

fn framed_segment(
    payload: &[u8],
    start: usize,
    end: usize,
    boundary_anchored: bool,
) -> Option<FramedRow> {
    let segment = payload.get(start..end)?;
    let mut prefixes = (0..segment.len())
        .filter_map(|row_start| {
            topology_prefix_fields(segment, row_start).map(|prefix| (row_start, prefix.end))
        })
        .collect::<Vec<_>>();
    prefixes.sort_unstable_by_key(|(_, end)| *end);
    let closes = segment
        .windows(3)
        .enumerate()
        .filter(|(_, bytes)| *bytes == [0, 0, 0xe3])
        .map(|(offset, _)| offset)
        .collect::<Vec<_>>();
    for close in closes.into_iter().rev() {
        let row_end = close + 3;
        let Some((suffix_start, _)) = topology_suffix(&segment[..row_end]) else {
            continue;
        };
        if boundary_anchored
            && topology_prefix_fields(segment, 0).is_some_and(|prefix| prefix.end <= suffix_start)
        {
            return Some(FramedRow {
                start,
                end: start + row_end,
            });
        }
        let eligible = prefixes.partition_point(|(_, prefix_end)| *prefix_end <= suffix_start);
        if eligible == 1 {
            return Some(FramedRow {
                start: start + prefixes[0].0,
                end: start + row_end,
            });
        }
    }
    None
}

fn framed_rows(payload: &[u8]) -> Vec<FramedRow> {
    let mut result = Vec::new();
    let mut arrays = Vec::new();
    let mut search = 0;
    while let Some(array) = find(payload, b"crv_array\0", search) {
        arrays.push(array + b"crv_array\0".len());
        search = array + b"crv_array\0".len();
    }
    if arrays.is_empty() {
        arrays.push(0);
    }
    for (index, &namespace_start) in arrays.iter().enumerate() {
        let namespace_end = arrays
            .get(index + 1)
            .map_or(payload.len(), |next| next - b"crv_array\0".len());
        let Some(label) = find_in(payload, b"topol_ref_data\0", namespace_start, namespace_end)
        else {
            continue;
        };
        let mut cursor = label + b"topol_ref_data\0".len();
        let mut boundary_anchored = false;
        while let Some((terminator, length)) = row_terminator(payload, cursor, namespace_end) {
            if let Some(row) = framed_segment(payload, cursor, terminator, boundary_anchored) {
                result.push(row);
            }
            cursor = terminator + length;
            boundary_anchored = true;
        }
    }
    result.sort_by_key(|row| row.start);
    result.dedup_by_key(|row| row.start);
    result
}

fn suffix_candidates(row: &[u8], body_start: usize, close: usize) -> Vec<usize> {
    let mut candidates = Vec::new();
    for length in 4..=11 {
        let Some(start) = close
            .checked_sub(length)
            .filter(|start| *start >= body_start)
        else {
            continue;
        };
        let Ok((_, p1)) = reference_id(row, start) else {
            continue;
        };
        let Ok((_, p2)) = reference_id(row, p1) else {
            continue;
        };
        let Ok((_, p3)) = reference_id(row, p2) else {
            continue;
        };
        let Ok((_, end)) = reference_id(row, p3) else {
            continue;
        };
        if end == close {
            candidates.push(start);
        }
    }
    candidates
}

fn curve_scalar_lane(
    body: &[u8],
    type_byte: u8,
    cache: &scalar::ScalarCache,
) -> (
    Vec<CurveParameterScalar>,
    Vec<CurveParameterReference>,
    Vec<CurveParameterOpaqueSpan>,
) {
    let mut scalars = Vec::new();
    let mut references = Vec::new();
    let mut claimed = vec![false; body.len()];
    let mut cursor = 0;
    while cursor < body.len() {
        if body[cursor] == psb::token::ENTITY_REF {
            if let Ok((reference, next)) = reference_id(body, cursor + 1) {
                references.push(CurveParameterReference {
                    entity_id: reference,
                    offset: cursor,
                    length: next - cursor,
                });
                claimed[cursor..next].fill(true);
                cursor = next;
                continue;
            }
        }
        if body[cursor] == 0x18
            && cursor + 1 == body.len()
            && matches!(type_byte, 0x00 | 0x01 | 0x06 | 0x08)
            && scalars.len() < 8
        {
            scalars.push(CurveParameterScalar {
                value: 0.0,
                raw: vec![0x18],
                offset: cursor,
                length: 1,
            });
            claimed[cursor] = true;
            cursor += 1;
            continue;
        }
        if let Some((value, next)) = scalar::decode_in_row_lane(body, cursor, cache) {
            scalars.push(CurveParameterScalar {
                value,
                raw: body[cursor..next].to_vec(),
                offset: cursor,
                length: next - cursor,
            });
            claimed[cursor..next].fill(true);
            cursor = next;
        } else {
            cursor += 1;
        }
    }
    let mut opaque_spans = Vec::new();
    let mut cursor = 0;
    while cursor < body.len() {
        if claimed[cursor] {
            cursor += 1;
            continue;
        }
        let start = cursor;
        while cursor < body.len() && !claimed[cursor] {
            cursor += 1;
        }
        opaque_spans.push(CurveParameterOpaqueSpan {
            raw: body[start..cursor].to_vec(),
            offset: start,
            length: cursor - start,
        });
    }
    (scalars, references, opaque_spans)
}

/// Decode analytic bodies from positional curve rows, retaining ambiguous
/// suffix boundaries without asserting topology connectivity.
pub fn parameter_records(payload: &[u8]) -> Vec<CurveParameterRecord> {
    let cache = scalar::ScalarCache::from_section(payload);
    let mut records = Vec::new();
    for framed in framed_rows(payload) {
        let row = &payload[framed.start..framed.end];
        let (curve_id, after_id) = compact_int(row, 0);
        let Some(&type_byte) = row.get(after_id) else {
            continue;
        };
        let (_, after_feature) = compact_int(row, after_id + 1);
        let body_start = after_feature + 2;
        let Some(close) = row.len().checked_sub(3) else {
            continue;
        };
        if row.get(close..) != Some(&[0, 0, 0xe3]) || body_start > close {
            continue;
        }
        let candidates = suffix_candidates(row, body_start, close);
        let Some(&suffix_start) = candidates.first() else {
            continue;
        };
        let body = row[body_start..suffix_start].to_vec();
        let (scalar_tokens, references, opaque_spans) = curve_scalar_lane(&body, type_byte, &cache);
        let scalar_values = scalar_tokens.iter().map(|token| token.value).collect();
        let skipped_references = references
            .iter()
            .map(|reference| reference.entity_id)
            .collect();
        records.push(CurveParameterRecord {
            curve_id,
            type_byte,
            body,
            scalar_values,
            scalar_tokens,
            skipped_references,
            references,
            opaque_spans,
            suffix: if candidates.len() == 1 {
                CurveSuffixStatus::Unique
            } else {
                CurveSuffixStatus::Ambiguous {
                    candidate_count: candidates.len(),
                }
            },
            offset: framed.start,
            body_offset: framed.start + body_start,
            suffix_offset: framed.start + suffix_start,
        });
    }
    records
}

fn uniquely_bounded_parameter_records(
    records: &[CurveParameterRecord],
) -> Vec<&CurveParameterRecord> {
    let mut counts = BTreeMap::new();
    for record in records {
        *counts.entry(record.curve_id).or_insert(0usize) += 1;
    }
    records
        .iter()
        .filter(|record| counts.get(&record.curve_id) == Some(&1))
        .filter(|record| record.suffix == CurveSuffixStatus::Unique)
        .collect()
}

/// Interpret complete eight-scalar parameter lanes for pcurve-family rows.
pub fn pcurve_endpoints(
    parameters: &[CurveParameterRecord],
    topology: &[CurveTopologyRow],
) -> Vec<PcurveEndpoints> {
    let mut result = uniquely_bounded_parameter_records(parameters)
        .into_iter()
        .filter(|record| matches!(record.type_byte, 0x00 | 0x01 | 0x06 | 0x08))
        .filter(|record| {
            record.scalar_tokens.len() == 8
                && record.references.is_empty()
                && record.opaque_spans.is_empty()
        })
        .filter_map(|record| {
            let mut matching = topology.iter().filter(|row| row.id == record.curve_id);
            let topology = matching.next()?;
            matching.next().is_none().then_some(())?;
            let values = &record.scalar_values;
            Some(PcurveEndpoints {
                curve_id: record.curve_id,
                faces: topology.faces,
                face_0_endpoints: [[values[0], values[1]], [values[4], values[5]]],
                face_1_endpoints: [[values[2], values[3]], [values[6], values[7]]],
                offset: record.offset,
            })
        })
        .collect::<Vec<_>>();
    result.sort_by_key(|record| record.offset);
    result
}

/// Decode exact world-coordinate tokens from FC-prefixed dense curve bodies.
pub fn fc_coordinates(parameters: &[CurveParameterRecord]) -> Vec<FcCurveCoordinates> {
    let mut result = Vec::new();
    for record in uniquely_bounded_parameter_records(parameters) {
        let Some((&0xfc, tail)) = record.body.split_first() else {
            continue;
        };
        let Some((&subtype, lane)) = tail.split_first() else {
            continue;
        };
        let mut tokens = Vec::new();
        let mut cursor = 0;
        while cursor < lane.len() {
            if matches!(lane[cursor], 0x46 | 0x2d) {
                if let Some((value, next)) = scalar::decode(lane, cursor) {
                    tokens.push(FcCurveCoordinateToken {
                        value_mm: value,
                        raw: lane[cursor..next].to_vec(),
                        offset: cursor + 2,
                        length: next - cursor,
                    });
                    cursor = next;
                    continue;
                }
            }
            cursor += 1;
        }
        if tokens.len() >= 4 {
            let mut opaque_spans = Vec::new();
            let mut unclaimed = 0;
            for token in &tokens {
                if unclaimed < token.offset {
                    opaque_spans.push(FcCurveOpaqueSpan {
                        raw: record.body[unclaimed..token.offset].to_vec(),
                        offset: unclaimed,
                        length: token.offset - unclaimed,
                    });
                }
                unclaimed = token.offset + token.length;
            }
            if unclaimed < record.body.len() {
                opaque_spans.push(FcCurveOpaqueSpan {
                    raw: record.body[unclaimed..].to_vec(),
                    offset: unclaimed,
                    length: record.body.len() - unclaimed,
                });
            }
            result.push(FcCurveCoordinates {
                curve_id: record.curve_id,
                subtype,
                body: record.body.clone(),
                values_mm: tokens.iter().map(|token| token.value_mm).collect(),
                tokens,
                opaque_spans,
                offset: record.offset,
            });
        }
    }
    result.sort_by_key(|record| record.offset);
    result
}

fn fc05_scalar(body: &[u8], offset: usize) -> Option<(f64, usize)> {
    let prefix = *body.get(offset)?;
    if prefix == 0x18 {
        return Some((0.0, offset + 1));
    }
    if let Some(decoded) = scalar::decode_positive_dict(body, offset) {
        return Some(decoded);
    }
    if let Some(decoded) = scalar::decode(body, offset) {
        return Some(decoded);
    }
    if matches!(prefix, 0xe0..=0xe3 | 0xf7 | 0xf8) || offset + 7 > body.len() {
        return None;
    }
    let byte_1 = prefix.wrapping_sub(0x8b);
    let mut raw = [0; 8];
    raw[0] = if byte_1 >= 0x80 { 0x3f } else { 0x40 };
    raw[1] = byte_1;
    raw[2..].copy_from_slice(&body[offset + 1..offset + 7]);
    Some((f64::from_be_bytes(raw), offset + 7))
}

/// Validate FC05 point lanes against their exact circle identity.
pub fn fc05_circles(parameters: &[CurveParameterRecord]) -> Vec<Fc05Circle> {
    let mut circles = Vec::new();
    for record in uniquely_bounded_parameter_records(parameters) {
        if record.body.get(..2) != Some(&[0xfc, 0x05]) {
            continue;
        }
        let mut points = Vec::new();
        let mut cursor = 2;
        while cursor < record.body.len() {
            if !matches!(record.body[cursor], 0x46 | 0x2d) {
                break;
            }
            let Some((x, next)) = fc05_scalar(&record.body, cursor) else {
                break;
            };
            let Some((z, next)) = fc05_scalar(&record.body, next) else {
                break;
            };
            let parameter_start = next;
            let Some((decoded_parameter, decoded_next)) = fc05_scalar(&record.body, next) else {
                break;
            };
            let (parameter, next) = if matches!(record.body.get(decoded_next), Some(0x46 | 0x2d)) {
                (Some(decoded_parameter), decoded_next)
            } else {
                let following = (parameter_start + 1..(parameter_start + 9).min(record.body.len()))
                    .find(|offset| matches!(record.body[*offset], 0x46 | 0x2d));
                let Some(following) = following else {
                    break;
                };
                (None, following)
            };
            let Some((ordinate, next)) = fc05_scalar(&record.body, next) else {
                break;
            };
            points.push((x, z, parameter, ordinate));
            cursor = next;
        }
        if cursor != record.body.len() && record.body.get(cursor..) != Some(&[0xff]) {
            continue;
        }
        if points.len() < 4 {
            continue;
        }
        let ordinate = points[0].3;
        if points.iter().any(|point| (point.3 - ordinate).abs() > 1e-9) {
            continue;
        }
        let first = points[0];
        let middle = points[points.len() / 2];
        let last = points[points.len() - 1];
        let a11 = 2.0 * (middle.0 - first.0);
        let a12 = 2.0 * (middle.1 - first.1);
        let a21 = 2.0 * (last.0 - middle.0);
        let a22 = 2.0 * (last.1 - middle.1);
        let determinant = a11.mul_add(a22, -(a12 * a21));
        if determinant.abs() < 1e-15 {
            continue;
        }
        let bx = middle.0.mul_add(middle.0, middle.1 * middle.1)
            - first.0.mul_add(first.0, first.1 * first.1);
        let bz = last.0.mul_add(last.0, last.1 * last.1)
            - middle.0.mul_add(middle.0, middle.1 * middle.1);
        let center_x = bx.mul_add(a22, -(a12 * bz)) / determinant;
        let center_z = a11.mul_add(bz, -(bx * a21)) / determinant;
        let radius = (first.0 - center_x).hypot(first.1 - center_z);
        if radius <= 0.0 {
            continue;
        }
        let residuals = points
            .iter()
            .map(|point| ((point.0 - center_x).hypot(point.1 - center_z) - radius).abs())
            .collect::<Vec<_>>();
        let max_residual = residuals.iter().copied().fold(0.0, f64::max);
        if max_residual > 1e-9 * radius.max(1.0) {
            continue;
        }
        let angle_0 = (first.1 - center_z).atan2(first.0 - center_x);
        let parameter_0 = first.2;
        let wrapped_distance = |left: f64, right: f64| {
            let difference = left - right;
            difference
                .is_finite()
                .then(|| difference.rem_euclid(std::f64::consts::TAU))
                .map_or(f64::INFINITY, |wrapped| {
                    wrapped.min(std::f64::consts::TAU - wrapped)
                })
        };
        let sign_matches = |sign: f64| {
            points.iter().all(|point| {
                let (Some(parameter), Some(parameter_0)) = (point.2, parameter_0) else {
                    return false;
                };
                let angle = (point.1 - center_z).atan2(point.0 - center_x);
                let expected = angle_0 + sign * (parameter - parameter_0);
                wrapped_distance(angle, expected) <= 1e-6
            })
        };
        let positive = sign_matches(1.0);
        let negative = sign_matches(-1.0);
        let angle_parameter_consistent = positive ^ negative;
        let parameter_sign = match (positive, negative) {
            (true, false) => Some(1),
            (false, true) => Some(-1),
            _ => None,
        };
        let reference_direction_row_frame =
            parameter_sign.zip(parameter_0).map(|(sign, parameter_0)| {
                let reference_angle = angle_0 - f64::from(sign) * parameter_0;
                [reference_angle.cos(), reference_angle.sin()]
            });
        let sample_direction_row_frame =
            [(first.0 - center_x) / radius, (first.1 - center_z) / radius];
        circles.push(Fc05Circle {
            curve_id: record.curve_id,
            center_row_frame: [center_x, center_z],
            radius_mm: radius,
            sample_direction_row_frame,
            reference_direction_row_frame,
            parameter_sign,
            cap_ordinate_row_frame: Some(ordinate),
            point_count: points.len(),
            max_residual,
            angle_parameter_consistent,
            offset: record.offset,
        });
    }
    circles.sort_by_key(|circle| circle.offset);
    circles
}

/// Bind validated `fc 05` circles to typed cylinder/plane face pairs and retain
/// only groups that agree on radius and center at two distinct cap ordinates.
pub fn fc05_cylinder_cap_pairs(
    circles: &[Fc05Circle],
    topology: &[CurveTopologyRow],
    surfaces: &[crate::surface::SurfaceRow],
) -> Vec<Fc05CylinderCapPair> {
    use std::collections::BTreeMap;

    let faces = crate::topology::uniquely_identified_rows(topology)
        .into_iter()
        .map(|row| (row.id, row.faces))
        .collect::<BTreeMap<_, _>>();
    let mut circle_counts = BTreeMap::<u32, usize>::new();
    for circle in circles {
        *circle_counts.entry(circle.curve_id).or_default() += 1;
    }
    let mut groups = BTreeMap::<u32, Vec<(&Fc05Circle, u32)>>::new();
    for circle in circles {
        if circle_counts.get(&circle.curve_id) != Some(&1) {
            continue;
        }
        let Some(adjacent) = faces.get(&circle.curve_id) else {
            continue;
        };
        let cylinders = adjacent
            .iter()
            .filter(|face| {
                crate::surface::unique_surface_row(surfaces, **face)
                    .is_some_and(|row| row.kind == crate::surface::SurfaceKind::Cylinder)
            })
            .copied()
            .collect::<Vec<_>>();
        let planes = adjacent
            .iter()
            .filter(|face| {
                crate::surface::unique_surface_row(surfaces, **face)
                    .is_some_and(|row| row.kind == crate::surface::SurfaceKind::Plane)
            })
            .copied()
            .collect::<Vec<_>>();
        if cylinders.len() == 1 && planes.len() == 1 && circle.cap_ordinate_row_frame.is_some() {
            groups
                .entry(cylinders[0])
                .or_default()
                .push((circle, planes[0]));
        }
    }

    let mut result = Vec::new();
    for (surface_id, mut group) in groups {
        group.sort_by_key(|(circle, _)| circle.offset);
        let first = group[0].0;
        let (Some(reference_direction_row_frame), Some(parameter_sign)) =
            (first.reference_direction_row_frame, first.parameter_sign)
        else {
            continue;
        };
        let tolerance = 1e-9 * first.radius_mm.max(1.0);
        if !group.iter().all(|(circle, _)| {
            (circle.radius_mm - first.radius_mm).abs() <= tolerance
                && (circle.center_row_frame[0] - first.center_row_frame[0]).abs() <= tolerance
                && (circle.center_row_frame[1] - first.center_row_frame[1]).abs() <= tolerance
                && circle.parameter_sign == first.parameter_sign
                && circle
                    .reference_direction_row_frame
                    .is_some_and(|direction| {
                        (direction[0] - reference_direction_row_frame[0]).abs() <= tolerance
                            && (direction[1] - reference_direction_row_frame[1]).abs() <= tolerance
                    })
                && circle.angle_parameter_consistent
        }) {
            continue;
        }
        let mut ordinates = Vec::new();
        for ordinate in group
            .iter()
            .filter_map(|(circle, _)| circle.cap_ordinate_row_frame)
        {
            if ordinates
                .iter()
                .all(|existing: &f64| (*existing - ordinate).abs() > tolerance)
            {
                ordinates.push(ordinate);
            }
        }
        if ordinates.len() < 2 {
            continue;
        }
        result.push(Fc05CylinderCapPair {
            surface_id,
            curve_ids: group.iter().map(|(circle, _)| circle.curve_id).collect(),
            cap_plane_ids: group.iter().map(|(_, plane)| *plane).collect(),
            curve_cap_ordinates_row_frame: group
                .iter()
                .filter_map(|(circle, _)| circle.cap_ordinate_row_frame)
                .collect(),
            center_row_frame: first.center_row_frame,
            radius_mm: first.radius_mm,
            reference_direction_row_frame,
            parameter_sign,
            cap_ordinates_row_frame: ordinates,
            offset: first.offset,
        });
    }
    result.sort_by_key(|pair| pair.offset);
    result
}

/// Decode labeled `crv_pnt_arr f9 02 04` prototype pcurve endpoints.
pub fn prototype_pcurve_endpoints(payload: &[u8]) -> Vec<PrototypePcurveEndpoints> {
    let cache = scalar::ScalarCache::from_section(payload);
    let mut result = Vec::new();
    let mut search = 0;
    while let Some(namespace) = find(payload, b"crv_array\0", search) {
        let start = namespace + b"crv_array\0".len();
        let end = find(payload, b"crv_array\0", start).unwrap_or(payload.len());
        search = start;
        let Some(id_label) = find_in(payload, b"crv_id\0", start, end) else {
            continue;
        };
        let id_start = id_label + b"crv_id\0".len();
        let (curve_id, after_id) = compact_int(payload, id_start);
        if after_id == id_start {
            continue;
        }
        let prototype_end = find_in(payload, b"topol_ref_data\0", after_id, end).unwrap_or(end);
        let Some(points_label) = find_in(payload, b"crv_pnt_arr\0", after_id, prototype_end) else {
            continue;
        };
        let search_end = (points_label + 64).min(prototype_end);
        let Some(header) = find_in(
            payload,
            &[psb::token::SCALAR_BODY, 0x02, 0x04],
            points_label,
            search_end,
        ) else {
            continue;
        };
        let mut values = Vec::with_capacity(8);
        let mut cursor = header + 3;
        while cursor < prototype_end && values.len() < 8 {
            if let Some((value, next)) = scalar::decode_in_lane(payload, cursor, &cache) {
                values.push(value);
                cursor = next;
            } else {
                cursor += 1;
            }
        }
        if values.len() == 8 {
            result.push(PrototypePcurveEndpoints {
                curve_id,
                face_0_endpoints: [[values[0], values[1]], [values[4], values[5]]],
                face_1_endpoints: [[values[2], values[3]], [values[6], values[7]]],
                offset: points_label,
            });
        }
    }
    result.sort_by_key(|record| record.offset);
    result
}

/// Decode the four labeled topology pointers of each curve prototype.
pub fn prototype_topology(payload: &[u8]) -> Vec<CurvePrototypeTopology> {
    let mut result = Vec::new();
    let mut search = 0;
    while let Some(namespace) = find(payload, b"crv_array\0", search) {
        let start = namespace + b"crv_array\0".len();
        let end = find(payload, b"crv_array\0", start).unwrap_or(payload.len());
        search = start;
        let Some(id_label) = find_in(payload, b"crv_id\0", start, end) else {
            continue;
        };
        let id_start = id_label + b"crv_id\0".len();
        let Ok((curve_id, _)) = reference_id(payload, id_start) else {
            continue;
        };
        let prototype_end = find_in(payload, b"topol_ref_data\0", id_start, end).unwrap_or(end);
        let reference = |label: &[u8]| {
            let at = find_in(payload, label, id_start, prototype_end)? + label.len();
            reference_id(payload, at).ok().map(|(value, _)| value)
        };
        let Some(face_0) = reference(b"crv_hdr_geom_ptr[0]\0") else {
            continue;
        };
        let Some(face_1) = reference(b"crv_hdr_geom_ptr[1]\0") else {
            continue;
        };
        let Some(next_0) = reference(b"next_crv_hdr_ptr[0]\0") else {
            continue;
        };
        let Some(next_1) = reference(b"next_crv_hdr_ptr[1]\0") else {
            continue;
        };
        result.push(CurvePrototypeTopology {
            curve_id,
            faces: [face_0, face_1],
            next_edges: [next_0, next_1],
            offset: namespace,
        });
    }
    result.sort_by_key(|record| record.offset);
    result
}

/// Bind complete prototype UV endpoints to labeled prototype topology.
pub fn bind_prototype_pcurves(
    pcurves: &[PrototypePcurveEndpoints],
    topology: &[CurvePrototypeTopology],
) -> Vec<BoundPrototypePcurve> {
    let mut pcurve_counts = BTreeMap::new();
    for pcurve in pcurves {
        *pcurve_counts.entry(pcurve.curve_id).or_insert(0usize) += 1;
    }
    let mut topology_counts = BTreeMap::new();
    for row in topology {
        *topology_counts.entry(row.curve_id).or_insert(0usize) += 1;
    }
    let mut result = pcurves
        .iter()
        .filter(|pcurve| pcurve_counts.get(&pcurve.curve_id) == Some(&1))
        .filter(|pcurve| topology_counts.get(&pcurve.curve_id) == Some(&1))
        .filter_map(|pcurve| {
            let topology = topology
                .iter()
                .find(|topology| topology.curve_id == pcurve.curve_id)?;
            Some(BoundPrototypePcurve {
                curve_id: pcurve.curve_id,
                faces: topology.faces,
                face_0_endpoints: pcurve.face_0_endpoints,
                face_1_endpoints: pcurve.face_1_endpoints,
                offset: pcurve.offset,
            })
        })
        .collect::<Vec<_>>();
    result.sort_by_key(|record| record.offset);
    result
}

fn parse_topology_row(row: &[u8], absolute_offset: usize) -> Option<CurveTopologyRow> {
    let (suffix_start, [f0, f1, e0, e1]) = topology_suffix(row)?;
    let prefix = topology_prefix(row, 0, suffix_start)?;
    Some(CurveTopologyRow {
        id: prefix.id,
        type_byte: prefix.type_byte,
        feature_id: prefix.feature_id,
        directions: prefix.directions,
        faces: [f0, f1],
        next_edges: [e0, e1],
        offset: absolute_offset,
    })
}

fn topology_prefix(row: &[u8], start: usize, suffix_start: usize) -> Option<TopologyPrefix> {
    let fields = topology_prefix_fields(row, start)?;
    (fields.end <= suffix_start).then_some(fields)
}

fn topology_prefix_fields(row: &[u8], start: usize) -> Option<TopologyPrefix> {
    let (id, after_id) = compact_int(row, start);
    (after_id > start).then_some(())?;
    let type_byte = *row.get(after_id)?;
    let (feature_id, after_feature) = compact_int(row, after_id + 1);
    (after_feature > after_id + 1).then_some(())?;
    let directions = [*row.get(after_feature)?, *row.get(after_feature + 1)?];
    directions
        .iter()
        .all(|direction| matches!(direction, 0x01 | 0xf6))
        .then_some(TopologyPrefix {
            id,
            type_byte,
            feature_id,
            directions,
            end: after_feature + 2,
        })
}

fn topology_suffix(row: &[u8]) -> Option<(usize, [u32; 4])> {
    let close = row.len().checked_sub(3)?;
    (row.get(close..)? == [0, 0, 0xe3]).then_some(())?;
    let mut candidates = Vec::new();
    for length in 4..=11 {
        let Some(start) = close.checked_sub(length) else {
            continue;
        };
        let Ok((f0, p1)) = reference_id(row, start) else {
            continue;
        };
        let Ok((f1, p2)) = reference_id(row, p1) else {
            continue;
        };
        let Ok((e0, p3)) = reference_id(row, p2) else {
            continue;
        };
        let Ok((e1, end)) = reference_id(row, p3) else {
            continue;
        };
        if end == close {
            candidates.push((start, [f0, f1, e0, e1]));
        }
    }
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(*candidate)
}

fn find(data: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    data.get(from..)?
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|relative| from + relative)
}

fn find_in(data: &[u8], needle: &[u8], from: usize, end: usize) -> Option<usize> {
    data.get(from..end)?
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|relative| from + relative)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn number(value: f64) -> Option<CurveExpressionValue> {
        Some(CurveExpressionValue::Number(value))
    }

    fn numeric_value(value: &Option<CurveExpressionValue>) -> f64 {
        let Some(CurveExpressionValue::Number(value)) = value else {
            panic!("expected evaluated numeric value")
        };
        *value
    }

    fn parameter_record(curve_id: u32, suffix: CurveSuffixStatus) -> CurveParameterRecord {
        CurveParameterRecord {
            curve_id,
            type_byte: 0,
            body: Vec::new(),
            scalar_values: Vec::new(),
            scalar_tokens: Vec::new(),
            skipped_references: Vec::new(),
            references: Vec::new(),
            opaque_spans: Vec::new(),
            suffix,
            offset: curve_id as usize,
            body_offset: curve_id as usize,
            suffix_offset: curve_id as usize,
        }
    }

    #[test]
    fn typed_parameter_rows_require_unique_identity_and_suffix_boundary() {
        let unique = parameter_record(7, CurveSuffixStatus::Unique);
        assert_eq!(
            uniquely_bounded_parameter_records(&[unique.clone()]).len(),
            1
        );

        let ambiguous = parameter_record(8, CurveSuffixStatus::Ambiguous { candidate_count: 2 });
        assert!(uniquely_bounded_parameter_records(&[ambiguous]).is_empty());
        assert!(uniquely_bounded_parameter_records(&[unique.clone(), unique]).is_empty());
    }

    #[test]
    fn finds_labeled_prototypes_in_concatenated_namespaces() {
        let payload = b"crv_array\0crv_id\0\x07type\0\x08feat_id\0\x04\
                       crv_array\0crv_id\0\x80\x80type\0\x01";
        assert_eq!(
            prototypes(payload),
            vec![
                CurvePrototype {
                    id: 7,
                    type_byte: 8,
                    feature_id: Some(4),
                    offset: 0,
                },
                CurvePrototype {
                    id: 128,
                    type_byte: 1,
                    feature_id: None,
                    offset: 33,
                },
            ]
        );
    }

    #[test]
    fn ignores_incomplete_labeled_rows() {
        assert!(prototypes(b"crv_array\0crv_id\0\x07").is_empty());
    }

    #[test]
    fn decodes_counted_curve_expression_source_lines() {
        let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x89\x4c\
            \xe0\x0aexpression\0\xf8\x04r=5\0theta=t*360\0z=71*t\0q=r+2*(3)\0\
            \xe0\x00backup_ents(crv_fr_eqn)\0\xe3\xe0\x01id\0\0\
            \xe0\x0aexpression\0\xf8\x01r=5\0";
        let records = expression_records(payload);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].entity_id, 0x094c);
        assert!(!records[0].backup);
        assert_eq!(
            records[0]
                .lines
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>(),
            ["r=5", "theta=t*360", "z=71*t", "q=r+2*(3)"]
        );
        assert!(records[1].backup);
        assert_eq!(records[1].lines[0].text, "r=5");
        assert!(records[0].lines[0].offset < records[0].lines[1].offset);
        assert_eq!(records[0].assignments.len(), 4);
        assert_eq!(records[0].assignments[0].name, "r");
        assert_eq!(records[0].assignments[0].expression, "5");
        assert!(records[0].assignments[0].dependencies.is_empty());
        assert_eq!(records[0].assignments[0].value, number(5.0));
        assert_eq!(records[0].assignments[1].name, "theta");
        assert_eq!(records[0].assignments[1].expression, "t*360");
        assert_eq!(records[0].assignments[1].dependencies, ["t"]);
        assert_eq!(records[0].assignments[1].value, None);
        assert_eq!(records[0].assignments[2].value, None);
        assert_eq!(records[0].assignments[3].dependencies, ["r"]);
        assert_eq!(records[0].assignments[3].value, number(11.0));
    }

    #[test]
    fn evaluates_creo_math_functions_without_treating_function_names_as_dependencies() {
        let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
            \xe0\x0aexpression\0\xf8\x05a=SIN(30)\0b=pow(a,2)+sqrt(9)\0\
            c=bound(12,0,10)+dead(3,1,2)\0d=custom(a)\0e=1e3\0";
        let records = expression_records(payload);
        let assignments = &records[0].assignments;

        assert!(assignments[0].dependencies.is_empty());
        assert!((numeric_value(&assignments[0].value) - 0.5).abs() < 1e-12);
        assert_eq!(assignments[1].dependencies, ["a"]);
        assert!((numeric_value(&assignments[1].value) - 3.25).abs() < 1e-12);
        assert!(assignments[2].dependencies.is_empty());
        assert_eq!(assignments[2].value, number(11.0));
        assert_eq!(assignments[3].dependencies, ["custom", "a"]);
        assert_eq!(assignments[3].value, None);
        assert!(assignments[4].dependencies.is_empty());
        assert_eq!(assignments[4].value, number(1000.0));

        let values = BTreeMap::new();
        let cases = [
            ("cos(60)", 0.5),
            ("tan(45)", 1.0),
            ("asin(1)", 90.0),
            ("acos(0)", 90.0),
            ("atan(1)", 45.0),
            ("atan2(1,0)", 90.0),
            ("sinh(0)", 0.0),
            ("cosh(0)", 1.0),
            ("tanh(0)", 0.0),
            ("sign(-2,-1)", -2.0),
            ("sign(-2,-0)", 2.0),
            ("mod(-5,3)", -2.0),
            ("if(0,2,3)", 3.0),
            ("near(2,2.1,0.2)", 1.0),
            ("min(2,3)+max(2,3)", 5.0),
            ("log(100)", 2.0),
            ("ln(exp(1))", 1.0),
            ("abs(-2)", 2.0),
            ("ceil(2.1)+floor(2.9)", 5.0),
            ("dbl_in_tol(2,2.1,0.2)", 1.0),
            ("2^3^2", 512.0),
            ("-2^2", -4.0),
            ("(-2)^2", 4.0),
            ("2^-2", 0.25),
            ("2+3*4==14", 1.0),
            ("2>=2 & 3<>4", 1.0),
            ("2<1 | 3~=4", 1.0),
            ("!(2<=3)", 0.0),
            ("~-1", 0.0),
            ("if(2^3==8,5,6)", 5.0),
        ];
        for (expression, expected) in cases {
            let actual = evaluate_expression(expression, &values).expect(expression);
            assert!((actual - expected).abs() < 1e-12, "{expression}");
        }
        assert_eq!(evaluate_expression("sqrt(-1)", &values), None);
        assert_eq!(evaluate_expression("sinh(86)", &values), None);
        assert_eq!(evaluate_expression("bound(1,2,1)", &values), None);
        assert_eq!(evaluate_expression("sin()", &values), None);
        assert_eq!(evaluate_expression("1<2<3", &values), None);
        let excessive_power_depth = format!("{}2", "2^".repeat(129));
        assert_eq!(evaluate_expression(&excessive_power_depth, &values), None);
        let long_unary_chain = format!("{}1", "-".repeat(1024));
        assert_eq!(evaluate_expression(&long_unary_chain, &values), Some(1.0));
    }

    #[test]
    fn evaluates_string_relations_and_ignores_literal_contents_in_dependencies() {
        let sources = [
            "material='steel'",
            "label=material+\"-\"+itos(2.4)",
            "where=search(label,'eel')",
            "piece=extract(label,2,3)",
            "length=string_length(piece)",
            "starts=string_starts(label,'ste')",
            "ends=string_ends(label,'-2')",
            "same=piece=='tee'",
            "matches=string_match(label,'steel-2')",
            "pattern=string_pattern(label,'steel-[0-9]*')",
            "not_pattern=string_pattern(label,'steel-[A-Z]*')",
            "zero=itos(0)",
            "bad=-'text'",
            "bad_pattern=string_pattern(label,'[')",
        ];
        let lines = sources
            .iter()
            .enumerate()
            .map(|(offset, text)| CurveExpressionLine {
                text: (*text).to_owned(),
                offset,
            })
            .collect::<Vec<_>>();
        let assignments =
            evaluate_expression_program(&lines, None, &ExternalRelationSymbols::default());

        assert!(assignments[0].dependencies.is_empty());
        assert_eq!(
            assignments[0].value,
            Some(CurveExpressionValue::String("steel".into()))
        );
        assert_eq!(assignments[1].dependencies, ["material"]);
        assert_eq!(
            assignments[1].value,
            Some(CurveExpressionValue::String("steel-2".into()))
        );
        assert_eq!(assignments[2].value, number(3.0));
        assert_eq!(
            assignments[3].value,
            Some(CurveExpressionValue::String("tee".into()))
        );
        assert_eq!(assignments[4].value, number(3.0));
        assert_eq!(assignments[5].value, number(1.0));
        assert_eq!(assignments[6].value, number(1.0));
        assert_eq!(assignments[7].value, number(1.0));
        assert_eq!(assignments[8].value, number(1.0));
        assert_eq!(assignments[9].value, number(1.0));
        assert_eq!(assignments[10].value, number(0.0));
        assert_eq!(
            assignments[11].value,
            Some(CurveExpressionValue::String(String::new()))
        );
        assert_eq!(assignments[12].value, None);
        assert_eq!(assignments[13].value, None);
    }

    #[test]
    fn formats_relation_reals_with_creo_rtos_conventions() {
        let values = BTreeMap::new();
        let cases = [
            ("rtos(123.456789)", "123.456789"),
            ("rtos(123.456789,3)", "123.457"),
            ("rtos(123.456789,4,YES)", "1.2346e02"),
            ("rtos(0)", ""),
            ("rtos(-0,3,YES)", ""),
            ("rtos(0.01234,2,TRUE)", "1.23e-02"),
            ("rel_model_type()", "part"),
        ];
        for (expression, expected) in cases {
            assert_eq!(
                evaluate_relation_expression(
                    expression,
                    &values,
                    RelationEvaluationContext::default()
                ),
                Some(CurveExpressionValue::String(expected.to_owned())),
                "{expression}"
            );
        }
        assert_eq!(
            evaluate_relation_expression(
                "rtos(1,-1)",
                &values,
                RelationEvaluationContext::default()
            ),
            None
        );
        assert_eq!(
            evaluate_relation_expression(
                "rtos(1,1.5)",
                &values,
                RelationEvaluationContext::default()
            ),
            None
        );
        assert_eq!(
            evaluate_relation_expression(
                "rtos(1,129)",
                &values,
                RelationEvaluationContext::default()
            ),
            None
        );
        assert_eq!(
            evaluate_relation_expression(
                "rtos(1,2,YES,NO)",
                &values,
                RelationEvaluationContext::default()
            ),
            None
        );
        assert_eq!(
            evaluate_relation_expression(
                "rel_model_name()",
                &values,
                RelationEvaluationContext {
                    model_name: Some("widget"),
                    ..RelationEvaluationContext::default()
                },
            ),
            Some(CurveExpressionValue::String("widget".to_owned()))
        );
        assert_eq!(
            evaluate_relation_expression(
                "rel_model_name()",
                &values,
                RelationEvaluationContext::default(),
            ),
            None
        );
    }

    #[test]
    fn proves_exists_for_local_and_external_relation_symbols() {
        let sources = [
            "IF exists('later')",
            "selected=1",
            "ELSE",
            "selected=2",
            "ENDIF",
            "later=5",
            "IF exists('d42')",
            "dimension=3",
            "ENDIF",
            "IF exists('external')",
            "unknown=1",
            "ENDIF",
        ];
        let lines = sources
            .iter()
            .enumerate()
            .map(|(offset, text)| CurveExpressionLine {
                text: (*text).to_owned(),
                offset,
            })
            .collect::<Vec<_>>();
        let mut external_symbols = ExternalRelationSymbols::default();
        external_symbols.observe("d42", None);
        let assignments = evaluate_expression_program(&lines, None, &external_symbols);

        assert_eq!(assignments.len(), 5);
        assert!(assignments[0].dependencies.is_empty());
        assert_eq!(assignments[0].activation, CurveExpressionActivation::Active);
        assert_eq!(assignments[0].value, number(1.0));
        assert_eq!(
            assignments[1].activation,
            CurveExpressionActivation::Inactive
        );
        assert_eq!(assignments[1].value, None);
        assert_eq!(assignments[2].value, number(5.0));
        assert_eq!(assignments[3].activation, CurveExpressionActivation::Active);
        assert_eq!(assignments[3].value, number(3.0));
        assert_eq!(
            assignments[4].activation,
            CurveExpressionActivation::Conditional
        );
        assert_eq!(assignments[4].value, None);
    }

    #[test]
    fn reevaluates_expression_records_after_external_symbols_are_decoded() {
        let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
            \xe0\x0aexpression\0\xf8\x03IF exists('d42')\0selected=1\0ENDIF\0";
        let mut records = expression_records(payload);

        assert_eq!(
            records[0].assignments[0].activation,
            CurveExpressionActivation::Conditional
        );
        let mut external_symbols = ExternalRelationSymbols::default();
        external_symbols.observe("d42", None);
        reevaluate_expression_records(&mut records, None, &external_symbols);
        assert_eq!(
            records[0].assignments[0].activation,
            CurveExpressionActivation::Active
        );
        assert_eq!(records[0].assignments[0].value, number(1.0));
    }

    #[test]
    fn external_symbol_values_require_agreeing_observations() {
        let lines = [CurveExpressionLine {
            text: "value=d42+1".to_owned(),
            offset: 0,
        }];
        let mut external_symbols = ExternalRelationSymbols::default();
        external_symbols.observe("D42", number(2.0));
        external_symbols.observe("d42", number(2.0));
        assert_eq!(
            evaluate_expression_program(&lines, None, &external_symbols)[0].value,
            number(3.0)
        );

        external_symbols.observe("d42", number(4.0));
        assert_eq!(
            evaluate_expression_program(&lines, None, &external_symbols)[0].value,
            None
        );
    }

    #[test]
    fn binds_relation_symbols_case_insensitively_and_preserves_scoped_dependencies() {
        let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
            \xe0\x0aexpression\0\xf8\x04Radius=5\0q=radius+PI\0\
            external=d1:2+PARAM:FID_20\0RADIUS=7\0";
        let assignments = &expression_records(payload)[0].assignments;

        assert_eq!(assignments[1].dependencies, ["radius"]);
        assert_eq!(assignments[1].value, number(5.0 + std::f64::consts::PI));
        assert_eq!(assignments[2].dependencies, ["d1:2", "PARAM:FID_20"]);
        assert_eq!(assignments[2].value, None);
        assert_eq!(assignments[3].value, number(7.0));
        assert_eq!(
            evaluate_expression("pi", &BTreeMap::new()),
            Some(std::f64::consts::PI)
        );
    }

    #[test]
    fn evaluates_nested_relation_conditionals_in_source_order() {
        let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
            \xe0\x0aexpression\0\xf8\x0eA=0\0IF a==0\0b=5\0IF NO\0c=1\0\
            ELSE\0c=b+1\0ENDIF\0ELSE\0b=10\0ENDIF\0a=5\0d=B\0iffy=9\0";
        let assignments = &expression_records(payload)[0].assignments;

        assert_eq!(assignments.len(), 8);
        assert_eq!(assignments[0].value, number(0.0));
        assert_eq!(assignments[1].value, number(5.0));
        assert_eq!(
            assignments[2].activation,
            CurveExpressionActivation::Inactive
        );
        assert_eq!(assignments[2].value, None);
        assert_eq!(assignments[3].value, number(6.0));
        assert_eq!(
            assignments[4].activation,
            CurveExpressionActivation::Inactive
        );
        assert_eq!(assignments[5].value, number(5.0));
        assert_eq!(assignments[6].value, number(5.0));
        assert_eq!(assignments[7].name, "iffy");
        assert_eq!(assignments[7].value, number(9.0));
    }

    #[test]
    fn unresolved_and_malformed_conditionals_do_not_choose_a_branch() {
        let unresolved = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
            \xe0\x0aexpression\0\xf8\x06IF external\0x=1\0ELSE\0x=2\0ENDIF\0y=x+1\0";
        let assignments = &expression_records(unresolved)[0].assignments;
        assert_eq!(assignments.len(), 3);
        assert!(assignments[..2]
            .iter()
            .all(|assignment| assignment.activation == CurveExpressionActivation::Conditional));
        assert_eq!(assignments[2].activation, CurveExpressionActivation::Active);
        assert_eq!(assignments[2].value, None);

        let malformed = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x08\
            \xe0\x0aexpression\0\xf8\x04IF YES\0x=1\0ELSE trailing\0ENDIF\0";
        let assignments = &expression_records(malformed)[0].assignments;
        assert_eq!(
            assignments[0].activation,
            CurveExpressionActivation::Conditional
        );
        assert_eq!(assignments[0].value, None);
    }

    #[test]
    fn unresolved_reassignment_invalidates_the_previous_scalar_value() {
        let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
            \xe0\x0aexpression\0\xf8\x04a=5\0b=a+1\0a=external\0c=a+1\0";
        let records = expression_records(payload);
        let assignments = &records[0].assignments;

        assert_eq!(assignments[0].value, number(5.0));
        assert_eq!(assignments[1].value, number(6.0));
        assert_eq!(assignments[2].value, None);
        assert_eq!(assignments[3].value, None);
    }

    #[test]
    fn decodes_only_complete_explicit_curve_expression_frames() {
        let complete = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
            \xe0\x02local_sys\0\xf9\x04\x03\x18\xe5\x0f\x0f\x0f\xe4\x0f\x0f\x0f\x0f\x0f\
            \xe0\x0aexpression\0\xf8\x01r=5\0";
        assert_eq!(
            expression_records(complete)[0]
                .local_system
                .as_ref()
                .and_then(|frame| frame.explicit_slots),
            Some([0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0])
        );

        let inherited = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x08\
            \xe0\x02local_sys\0\xf9\x04\x03\x18\xe4\x0f\xe4\x18\xe5\x0f\x18\xe6\
            \xe0\x0aexpression\0\xf8\x01r=5\0";
        assert_eq!(
            expression_records(inherited)[0]
                .local_system
                .as_ref()
                .and_then(|frame| frame.explicit_slots),
            Some([0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0])
        );
    }

    #[test]
    fn decodes_compact_curve_expression_frame_extents() {
        let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
            \xe0\x02local_sys\0\xf9\x80\x88\x03\x0f\
            \xe0\x0aexpression\0\xf8\x01r=5\0";
        let records = expression_records(payload);
        let frame = records[0].local_system.as_ref().expect("local system");
        assert_eq!(frame.dimensions, 136);
        assert_eq!(frame.count, 3);
        assert_eq!(frame.body, [0x0f]);
        assert_eq!(frame.explicit_slots, None);
    }

    #[test]
    fn recognizes_only_affine_cylindrical_helix_programs() {
        let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
            \xe0\x0aexpression\0\xf8\x05unused=external\0r=5\0theta=90+t*720\0z=-2+20*t\0note=external+1\0";
        let records = expression_records(payload);
        assert_eq!(
            expression_helix(&records[0]),
            Some(CurveExpressionHelix {
                radius: 5.0,
                height: 20.0,
                z_start: -2.0,
                revolutions: 2.0,
                start_angle: std::f64::consts::FRAC_PI_2,
                clockwise: false,
            })
        );

        let constant_functions = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x08\
            \xe0\x0aexpression\0\xf8\x03r=sqrt(25)\0theta=atan(1)+t*360\0z=t*pow(2,3)\0";
        assert_eq!(
            expression_helix(&expression_records(constant_functions)[0]),
            Some(CurveExpressionHelix {
                radius: 5.0,
                height: 8.0,
                z_start: 0.0,
                revolutions: 1.0,
                start_angle: std::f64::consts::FRAC_PI_4,
                clockwise: false,
            })
        );

        let identity_powers = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x08\
            \xe0\x0aexpression\0\xf8\x03r=5^1\0theta=t^1*360\0z=8*t^1\0";
        assert_eq!(
            expression_helix(&expression_records(identity_powers)[0]),
            Some(CurveExpressionHelix {
                radius: 5.0,
                height: 8.0,
                z_start: 0.0,
                revolutions: 1.0,
                start_angle: 0.0,
                clockwise: false,
            })
        );

        let nonlinear = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x08\
            \xe0\x0aexpression\0\xf8\x03r=5\0theta=t*t*360\0z=20*t\0";
        assert!(expression_helix(&expression_records(nonlinear)[0]).is_none());

        let sample_alias = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x09\
            \xe0\x0aexpression\0\xf8\x03r=5\0theta=360*t+t*(t-0.5)*(t-1)\0z=20*t\0";
        assert!(expression_helix(&expression_records(sample_alias)[0]).is_none());
    }

    #[test]
    fn decodes_a_uniquely_delimited_topology_suffix() {
        let payload = [
            b't', b'o', b'p', b'o', b'l', b'_', b'r', b'e', b'f', b'_', b'd', b'a', b't', b'a', 0,
            7, 8, 4, 1, 0xf6, 0x29, 0x43, 0, // opaque row body
            10, 11, 7, 7, 0, 0, 0xe3, 0xe1, 0xe3,
        ];
        assert_eq!(
            topology_rows(&payload),
            vec![CurveTopologyRow {
                id: 7,
                type_byte: 8,
                feature_id: 4,
                directions: [1, 0xf6],
                faces: [10, 11],
                next_edges: [7, 7],
                offset: 15,
            }]
        );
    }

    #[test]
    fn row_boundary_outweighs_prefix_like_bytes_inside_a_dense_body() {
        let payload = [
            b't', b'o', b'p', b'o', b'l', b'_', b'r', b'e', b'f', b'_', b'd', b'a', b't', b'a', 0,
            0xff, 0xe1, 0xe3, // named prototype segment
            7, 8, 4, 1, 0xf6, // row prefix
            0xfc, 5, 9, 8, 4, 1, 0xf6, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
            0xff, 0xff, 0xff, // dense body with a false prefix
            10, 11, 7, 7, 0, 0, 0xe3, 0xe1, 0xe3,
        ];

        assert_eq!(topology_rows(&payload).len(), 1);
        assert_eq!(
            parameter_records(&payload)[0].body[0..7],
            [0xfc, 5, 9, 8, 4, 1, 0xf6]
        );
    }

    #[test]
    fn decodes_complete_depdb_one_sided_curve_array() {
        let payload = b"crv_array\0\xf2\xf8\x02crv_id\0\x06type\0\x08feat_id\0\x04topol_ref_data\0\x07\x08\x04\x01\xf6\xe4\xff\0\x09\x0a\0\xe1\xe0next_record\0";

        let rows = depdb_cross_section_rows(payload);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, 7);
        assert_eq!(rows[0].type_byte, 8);
        assert_eq!(rows[0].feature_id, 4);
        assert_eq!(rows[0].directions, [1, 0xf6]);
        assert_eq!(rows[0].suffix, [0, 9, 10, 0]);
        assert_eq!(rows[0].body, [0xe4, 0xff]);
        assert_eq!(rows[0].scalar_tokens.len(), 1);
        assert_eq!(rows[0].scalar_tokens[0].value, 1.0);
        assert_eq!(rows[0].opaque_spans.len(), 1);
        assert_eq!(rows[0].opaque_spans[0].raw, [0xff]);
    }

    #[test]
    fn row_terminator_selects_the_first_short_or_long_marker() {
        let short_then_long = [0xe1, 0xe3, 0, 0xe1, 0xf5, 0x05, 0xf6, 0xe3];
        assert_eq!(
            row_terminator(&short_then_long, 0, short_then_long.len()),
            Some((0, 2))
        );
        let long_then_short = [0xe1, 0xf5, 0x05, 0xf6, 0xe3, 0, 0xe1, 0xe3];
        assert_eq!(
            row_terminator(&long_then_short, 0, long_then_short.len()),
            Some((0, 5))
        );
    }

    #[test]
    fn binds_agreeing_fc05_caps_to_one_typed_cylinder() {
        let circle = |curve_id, ordinate, offset| Fc05Circle {
            curve_id,
            center_row_frame: [3.0, 4.0],
            radius_mm: 2.0,
            sample_direction_row_frame: [1.0, 0.0],
            reference_direction_row_frame: Some([1.0, 0.0]),
            parameter_sign: Some(1),
            cap_ordinate_row_frame: Some(ordinate),
            point_count: 8,
            max_residual: 0.0,
            angle_parameter_consistent: true,
            offset,
        };
        let topology = |curve_id, plane_id, offset| CurveTopologyRow {
            id: curve_id,
            type_byte: 5,
            feature_id: 4,
            directions: [1, 0xf6],
            faces: [10, plane_id],
            next_edges: [curve_id, curve_id],
            offset,
        };
        let surface = |id, kind: crate::surface::SurfaceKind| crate::surface::SurfaceRow {
            id,
            type_byte: kind.canonical_type_byte(),
            kind,
            feature_id: 4,
            reversed: false,
            boundary_type: 0,
            next_surface: 0,
            offset: usize::try_from(id).expect("fixture id fits usize"),
        };
        let pairs = fc05_cylinder_cap_pairs(
            &[circle(20, -5.0, 100), circle(21, 7.0, 200)],
            &[topology(20, 11, 100), topology(21, 12, 200)],
            &[
                surface(10, crate::surface::SurfaceKind::Cylinder),
                surface(11, crate::surface::SurfaceKind::Plane),
                surface(12, crate::surface::SurfaceKind::Plane),
            ],
        );

        assert_eq!(
            pairs,
            vec![Fc05CylinderCapPair {
                surface_id: 10,
                curve_ids: vec![20, 21],
                cap_plane_ids: vec![11, 12],
                curve_cap_ordinates_row_frame: vec![-5.0, 7.0],
                center_row_frame: [3.0, 4.0],
                radius_mm: 2.0,
                reference_direction_row_frame: [1.0, 0.0],
                parameter_sign: 1,
                cap_ordinates_row_frame: vec![-5.0, 7.0],
                offset: 100,
            }]
        );
    }

    #[test]
    fn fc05_cap_pairs_require_unique_topology_and_surface_identities() {
        let circle = |curve_id, ordinate, offset| Fc05Circle {
            curve_id,
            center_row_frame: [3.0, 4.0],
            radius_mm: 2.0,
            sample_direction_row_frame: [1.0, 0.0],
            reference_direction_row_frame: Some([1.0, 0.0]),
            parameter_sign: Some(1),
            cap_ordinate_row_frame: Some(ordinate),
            point_count: 8,
            max_residual: 0.0,
            angle_parameter_consistent: true,
            offset,
        };
        let topology = |curve_id, plane_id, offset| CurveTopologyRow {
            id: curve_id,
            type_byte: 5,
            feature_id: 4,
            directions: [1, 0xf6],
            faces: [10, plane_id],
            next_edges: [curve_id, curve_id],
            offset,
        };
        let surface = |id, kind: crate::surface::SurfaceKind, offset| crate::surface::SurfaceRow {
            id,
            type_byte: kind.canonical_type_byte(),
            kind,
            feature_id: 4,
            reversed: false,
            boundary_type: 0,
            next_surface: 0,
            offset,
        };
        let circles = [circle(20, -5.0, 100), circle(21, 7.0, 200)];
        let topology_rows = [topology(20, 11, 100), topology(21, 12, 200)];
        let surfaces = [
            surface(10, crate::surface::SurfaceKind::Cylinder, 10),
            surface(11, crate::surface::SurfaceKind::Plane, 11),
            surface(12, crate::surface::SurfaceKind::Plane, 12),
        ];

        let mut duplicate_topology = topology_rows.to_vec();
        duplicate_topology.push(topology(20, 11, 300));
        assert!(fc05_cylinder_cap_pairs(&circles, &duplicate_topology, &surfaces).is_empty());

        let mut duplicate_surfaces = surfaces.to_vec();
        duplicate_surfaces.push(surface(10, crate::surface::SurfaceKind::Cylinder, 20));
        assert!(fc05_cylinder_cap_pairs(&circles, &topology_rows, &duplicate_surfaces).is_empty());

        let duplicate_circles = [
            circle(20, -5.0, 100),
            circle(20, 7.0, 150),
            circle(21, 7.0, 200),
        ];
        assert!(fc05_cylinder_cap_pairs(&duplicate_circles, &topology_rows, &surfaces).is_empty());
    }

    #[test]
    fn decodes_fc05_two_near_lane() {
        let bytes = [0x8b, 0x13, 0x11, 0x71, 0x7e, 0xcd, 0xf4];
        assert_eq!(
            fc05_scalar(&bytes, 0),
            Some((
                f64::from_be_bytes([0x40, 0x00, 0x13, 0x11, 0x71, 0x7e, 0xcd, 0xf4]),
                7
            ))
        );
        let lower = [0x71, 0x68, 0xf7, 0x91, 0x89, 0x97, 0x45, 0x2d];
        assert_eq!(
            fc05_scalar(&lower, 0),
            Some((
                f64::from_be_bytes([0x3f, 0xe6, 0x68, 0xf7, 0x91, 0x89, 0x97, 0x45]),
                7
            ))
        );
        let upper = [0xa3, 0x36, 0x6d, 0x17, 0x70, 0xe4, 0xb3];
        assert_eq!(
            fc05_scalar(&upper, 0),
            Some((
                f64::from_be_bytes([0x40, 0x18, 0x36, 0x6d, 0x17, 0x70, 0xe4, 0xb3]),
                7
            ))
        );
    }

    #[test]
    fn withholds_fc05_caps_without_distinct_ordinates() {
        let circles = [Fc05Circle {
            curve_id: 20,
            center_row_frame: [3.0, 4.0],
            radius_mm: 2.0,
            sample_direction_row_frame: [1.0, 0.0],
            reference_direction_row_frame: Some([1.0, 0.0]),
            parameter_sign: Some(1),
            cap_ordinate_row_frame: Some(5.0),
            point_count: 8,
            max_residual: 0.0,
            angle_parameter_consistent: true,
            offset: 100,
        }];
        assert!(fc05_cylinder_cap_pairs(&circles, &[], &[]).is_empty());
    }
}

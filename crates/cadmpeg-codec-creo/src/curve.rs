// SPDX-License-Identifier: Apache-2.0
//! Curve namespace prototypes and topology rows.
//!
//! Prototype rows identify curves and their generating features. Topology rows
//! add the two face sides and successor curve for each native half-edge. Curve
//! parameter bodies are not interpreted here.

use std::collections::BTreeMap;

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
    /// Sequentially evaluated scalar when every dependency is resolved.
    pub value: Option<f64>,
    /// Byte offset of the assignment source line.
    pub offset: usize,
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
                    .then(|| scalar::decode_curve_expression_local_system_slots(&body, &cache))
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
            let mut values = BTreeMap::new();
            let assignments = lines
                .iter()
                .filter_map(expression_assignment)
                .map(|mut assignment| {
                    assignment.value = evaluate_expression(&assignment.expression, &values);
                    if let Some(value) = assignment.value {
                        values.insert(assignment.name.clone(), value);
                    }
                    assignment
                })
                .collect();
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
    let mut dependencies = Vec::new();
    let bytes = expression.as_bytes();
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor] == b'_' || bytes[cursor].is_ascii_alphabetic() {
            let start = cursor;
            cursor += 1;
            while cursor < bytes.len()
                && (bytes[cursor] == b'_'
                    || bytes[cursor].is_ascii_alphabetic()
                    || bytes[cursor].is_ascii_digit())
            {
                cursor += 1;
            }
            let dependency = &expression[start..cursor];
            if !dependencies.iter().any(|existing| existing == dependency) {
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
        offset: line.offset,
    })
}

trait ArithmeticValue: Copy {
    fn number(value: f64) -> Self;
    fn add(self, right: Self) -> Option<Self>;
    fn subtract(self, right: Self) -> Option<Self>;
    fn multiply(self, right: Self) -> Option<Self>;
    fn divide(self, right: Self) -> Option<Self>;
    fn negate(self) -> Self;
    fn finite(self) -> bool;
}

impl ArithmeticValue for f64 {
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

    fn negate(self) -> Self {
        -self
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

impl ArithmeticValue for AffineValue {
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

    fn negate(self) -> Self {
        Self {
            constant: -self.constant,
            linear: -self.linear,
        }
    }

    fn finite(self) -> bool {
        self.constant.is_finite() && self.linear.is_finite()
    }
}

struct ArithmeticParser<'a, V> {
    source: &'a [u8],
    cursor: usize,
    values: &'a BTreeMap<String, V>,
    nesting: usize,
}

impl<V: ArithmeticValue> ArithmeticParser<'_, V> {
    fn whitespace(&mut self) {
        while self
            .source
            .get(self.cursor)
            .is_some_and(u8::is_ascii_whitespace)
        {
            self.cursor += 1;
        }
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
        let mut value = self.factor()?;
        loop {
            self.whitespace();
            match self.source.get(self.cursor) {
                Some(b'*') => {
                    self.cursor += 1;
                    value = value.multiply(self.factor()?)?;
                }
                Some(b'/') => {
                    self.cursor += 1;
                    value = value.divide(self.factor()?)?;
                }
                _ => return Some(value),
            }
        }
    }

    fn factor(&mut self) -> Option<V> {
        self.whitespace();
        let mut negate = false;
        while let Some(sign @ (b'+' | b'-')) = self.source.get(self.cursor) {
            negate ^= *sign == b'-';
            self.cursor += 1;
            self.whitespace();
        }
        let value = match self.source.get(self.cursor)? {
            b'(' => {
                const MAX_NESTING: usize = 128;
                (self.nesting < MAX_NESTING).then_some(())?;
                self.cursor += 1;
                self.nesting += 1;
                let value = self.expression()?;
                self.nesting -= 1;
                self.whitespace();
                (self.source.get(self.cursor) == Some(&b')')).then(|| {
                    self.cursor += 1;
                    value
                })
            }
            byte if byte.is_ascii_digit() || *byte == b'.' => self.number(),
            byte if byte.is_ascii_alphabetic() || *byte == b'_' => self.identifier(),
            _ => None,
        }?;
        Some(if negate { value.negate() } else { value })
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

    fn identifier(&mut self) -> Option<V> {
        let start = self.cursor;
        self.cursor += 1;
        while self.source.get(self.cursor).is_some_and(|byte| {
            byte.is_ascii_alphabetic() || byte.is_ascii_digit() || *byte == b'_'
        }) {
            self.cursor += 1;
        }
        let name = std::str::from_utf8(&self.source[start..self.cursor]).ok()?;
        self.values.get(name).copied()
    }
}

fn evaluate_expression(expression: &str, values: &BTreeMap<String, f64>) -> Option<f64> {
    let mut parser = ArithmeticParser {
        source: expression.as_bytes(),
        cursor: 0,
        values,
        nesting: 0,
    };
    let value = parser.expression()?;
    parser.whitespace();
    (parser.cursor == parser.source.len() && value.finite()).then_some(value)
}

fn evaluate_affine_expression(
    expression: &str,
    values: &BTreeMap<String, AffineValue>,
) -> Option<AffineValue> {
    let mut parser = ArithmeticParser {
        source: expression.as_bytes(),
        cursor: 0,
        values,
        nesting: 0,
    };
    let value = parser.expression()?;
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
        if let Some(value) = evaluate_affine_expression(&assignment.expression, &values) {
            values.insert(assignment.name.clone(), value);
        } else {
            values.remove(&assignment.name);
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

fn framed_segment(payload: &[u8], start: usize, end: usize) -> Option<FramedRow> {
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
        while let Some((terminator, length)) = row_terminator(payload, cursor, namespace_end) {
            if let Some(row) = framed_segment(payload, cursor, terminator) {
                result.push(row);
            }
            cursor = terminator + length;
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

/// Interpret complete eight-scalar parameter lanes for pcurve-family rows.
pub fn pcurve_endpoints(
    parameters: &[CurveParameterRecord],
    topology: &[CurveTopologyRow],
) -> Vec<PcurveEndpoints> {
    let mut result = parameters
        .iter()
        .filter(|record| matches!(record.type_byte, 0x00 | 0x01 | 0x06 | 0x08))
        .filter(|record| {
            record.scalar_tokens.len() == 8
                && record.references.is_empty()
                && record.opaque_spans.is_empty()
        })
        .filter_map(|record| {
            let topology = topology.iter().find(|row| row.id == record.curve_id)?;
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
    for record in parameters {
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
    if prefix == 0x8b {
        let tail = body.get(offset + 1..offset + 7)?;
        let mut raw = [0; 8];
        raw[..2].copy_from_slice(&[0x40, 0x00]);
        raw[2..].copy_from_slice(tail);
        return Some((f64::from_be_bytes(raw), offset + 7));
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
    for record in parameters {
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
            let Some((parameter, next)) = fc05_scalar(&record.body, next) else {
                break;
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
                let angle = (point.1 - center_z).atan2(point.0 - center_x);
                let expected = angle_0 + sign * (point.2 - parameter_0);
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
        let reference_direction_row_frame = parameter_sign.map(|sign| {
            let reference_angle = angle_0 - f64::from(sign) * parameter_0;
            [reference_angle.cos(), reference_angle.sin()]
        });
        circles.push(Fc05Circle {
            curve_id: record.curve_id,
            center_row_frame: [center_x, center_z],
            radius_mm: radius,
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

    let kinds = surfaces
        .iter()
        .map(|surface| (surface.id, surface.kind))
        .collect::<BTreeMap<_, _>>();
    let faces = topology
        .iter()
        .map(|row| (row.id, row.faces))
        .collect::<BTreeMap<_, _>>();
    let mut groups = BTreeMap::<u32, Vec<(&Fc05Circle, u32)>>::new();
    for circle in circles {
        let Some(adjacent) = faces.get(&circle.curve_id) else {
            continue;
        };
        let cylinders = adjacent
            .iter()
            .filter(|face| kinds.get(face) == Some(&crate::surface::SurfaceKind::Cylinder))
            .copied()
            .collect::<Vec<_>>();
        let planes = adjacent
            .iter()
            .filter(|face| kinds.get(face) == Some(&crate::surface::SurfaceKind::Plane))
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
    let mut result = pcurves
        .iter()
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
        assert_eq!(records[0].assignments[0].value, Some(5.0));
        assert_eq!(records[0].assignments[1].name, "theta");
        assert_eq!(records[0].assignments[1].expression, "t*360");
        assert_eq!(records[0].assignments[1].dependencies, ["t"]);
        assert_eq!(records[0].assignments[1].value, None);
        assert_eq!(records[0].assignments[2].value, None);
        assert_eq!(records[0].assignments[3].dependencies, ["r"]);
        assert_eq!(records[0].assignments[3].value, Some(11.0));
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
    fn decodes_fc05_two_near_lane() {
        let bytes = [0x8b, 0x13, 0x11, 0x71, 0x7e, 0xcd, 0xf4];
        assert_eq!(
            fc05_scalar(&bytes, 0),
            Some((
                f64::from_be_bytes([0x40, 0x00, 0x13, 0x11, 0x71, 0x7e, 0xcd, 0xf4]),
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

// SPDX-License-Identifier: Apache-2.0
//! The measurement report and its rows.
//!
//! Every type serializes its members in declaration order, which is the
//! order of the output contract. Maps are ordered by key. Fields are private;
//! the crate builds rows and readers use the accessors.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::reason::{Category, CaveatCode};
use crate::select::SurfaceKind;
use crate::value::{BoxValue, Point, Quantity, Range, Scalar, Sweep, Tensor, Unavailable};

/// Read accessors for private report fields.
macro_rules! accessors {
    ($ty:ident { $($(#[$doc:meta])* $name:ident: $ret:ty;)* }) => {
        impl $ty {
            $(
                $(#[$doc])*
                #[must_use]
                pub fn $name(&self) -> &$ret {
                    &self.$name
                }
            )*
        }
    };
}

/// The result of one measurement pass.
#[derive(Debug, Clone, Serialize)]
pub struct MeasureReport {
    pub(crate) tolerances: ReportTolerances,
    pub(crate) caveats: Vec<Caveat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) occurrence: Option<OccurrenceMeasures>,
    pub(crate) bodies: Vec<BodyMeasures>,
    pub(crate) faces: Vec<FaceMeasures>,
    pub(crate) edges: Vec<EdgeMeasures>,
    pub(crate) surfaces: Vec<SurfaceMeasures>,
    pub(crate) document: DocumentMeasures,
}

accessors!(MeasureReport {
    /// The document tolerances.
    tolerances: ReportTolerances;
    /// Document-level caveats.
    caveats: Vec<Caveat>;
    /// The occurrence whose instance space the rows are in; absent for
    /// model space.
    occurrence: Option<OccurrenceMeasures>;
    /// Body rows, in ID order.
    bodies: Vec<BodyMeasures>;
    /// Face rows, in ID order.
    faces: Vec<FaceMeasures>;
    /// Edge rows, in ID order.
    edges: Vec<EdgeMeasures>;
    /// Surface rows, in ID order.
    surfaces: Vec<SurfaceMeasures>;
    /// Document totals.
    document: DocumentMeasures;
});

/// The document tolerances, in millimetres and radians.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct ReportTolerances {
    pub(crate) linear: f64,
    pub(crate) angular: f64,
}

impl ReportTolerances {
    /// Linear tolerance in millimetres.
    #[must_use]
    pub const fn linear(&self) -> f64 {
        self.linear
    }

    /// Angular tolerance in radians.
    #[must_use]
    pub const fn angular(&self) -> f64 {
        self.angular
    }
}

/// A fact about a measured result that a reader should see.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Caveat {
    pub(crate) code: CaveatCode,
    pub(crate) category: Category,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) entity: Option<String>,
    pub(crate) message: String,
}

impl Caveat {
    pub(crate) fn new(code: CaveatCode, entity: Option<&str>, message: impl Into<String>) -> Self {
        Self {
            code,
            category: code.category(),
            entity: entity.map(str::to_owned),
            message: message.into(),
        }
    }

    /// The pinned code.
    #[must_use]
    pub const fn code(&self) -> CaveatCode {
        self.code
    }

    /// The code's category.
    #[must_use]
    pub const fn category(&self) -> Category {
        self.category
    }

    /// The entity the caveat names.
    #[must_use]
    pub fn entity(&self) -> Option<&str> {
        self.entity.as_deref()
    }
}

/// The class of a composed placement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlacementClass {
    /// No placement, or the identity transform bit for bit.
    Identity,
    /// A rigid motion, with or without reflection.
    Rigid,
    /// A uniform scale with a rigid motion.
    Similarity,
    /// Any other affine map.
    Affine,
}

/// The kind of an edge's curve, as edge rows report it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CurveKind {
    /// A line.
    Line,
    /// A circle.
    Circle,
    /// An ellipse.
    Ellipse,
    /// A parabola.
    Parabola,
    /// A hyperbola.
    Hyperbola,
    /// A NURBS curve.
    Nurbs,
    /// A polyline.
    Polyline,
    /// A composite curve.
    Composite,
    /// A procedural curve.
    Procedural,
    /// A curve collapsed to a point.
    Degenerate,
    /// An unknown curve.
    Unknown,
    /// The edge has no curve.
    None,
}

/// Whether every applicable quantity of a row was measured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RowStatus {
    /// Every applicable quantity was measured; `undefined` values do not
    /// count.
    Measured,
    /// Some quantities are unavailable.
    Partial,
    /// No quantity was measured.
    Unavailable,
}

/// Visits every quantity of a row.
pub(crate) trait QuantityVisitor {
    fn visit<T>(&mut self, quantity: &Quantity<T>);

    fn visit_option<T>(&mut self, quantity: Option<&Quantity<T>>) {
        if let Some(quantity) = quantity {
            self.visit(quantity);
        }
    }
}

/// Counts measured and unavailable quantities of one row.
#[derive(Debug, Default)]
pub(crate) struct StatusTally {
    measured: usize,
    unavailable: usize,
}

impl QuantityVisitor for StatusTally {
    fn visit<T>(&mut self, quantity: &Quantity<T>) {
        match quantity.unavailable_category() {
            None => self.measured += 1,
            Some(Category::Undefined) => {}
            Some(_) => self.unavailable += 1,
        }
    }
}

impl StatusTally {
    pub(crate) fn status(&self) -> RowStatus {
        match (self.measured, self.unavailable) {
            (_, 0) => RowStatus::Measured,
            (0, _) => RowStatus::Unavailable,
            _ => RowStatus::Partial,
        }
    }
}

/// Counts unavailable values by category and code.
#[derive(Debug, Default)]
pub(crate) struct Histogram(pub(crate) BTreeMap<Category, BTreeMap<String, usize>>);

impl Histogram {
    /// Count one unavailable value.
    pub(crate) fn count<T>(&mut self, unavailable: &Unavailable<T>) {
        *self
            .0
            .entry(unavailable.category())
            .or_default()
            .entry(unavailable.reason().code().to_owned())
            .or_default() += 1;
    }
}

impl QuantityVisitor for Histogram {
    fn visit<T>(&mut self, quantity: &Quantity<T>) {
        if let Some(unavailable) = quantity.unavailable() {
            self.count(unavailable);
        }
    }
}

/// The carrier of a face row.
#[derive(Debug, Clone, Serialize)]
pub struct CarrierMeasures {
    pub(crate) surface: String,
    pub(crate) kind: SurfaceKind,
    pub(crate) chain: Vec<String>,
    pub(crate) analytic: bool,
    pub(crate) placement: PlacementReport,
    pub(crate) parameters: CarrierParameters,
}

accessors!(CarrierMeasures {
    /// The surface ID.
    surface: String;
    /// The canonical kind.
    kind: SurfaceKind;
    /// The carrier chain, outer to inner.
    chain: Vec<String>;
    /// Whether the carrier is analytic. Informational.
    analytic: bool;
    /// The composed placement of the carrier chain and the body.
    placement: PlacementReport;
    /// Kind-specific parameters.
    parameters: CarrierParameters;
});

/// The space a row is measured in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "space", rename_all = "snake_case")]
pub enum RowPlacement {
    /// Model space: each body placed by its own transform.
    Model {},
    /// The instance space of an occurrence: each body placed by its own
    /// transform, then by the occurrence chain from the root.
    Occurrence {
        /// The occurrence ID.
        occurrence: String,
    },
    /// The surface as stored: no face uses it, so no body places it.
    Stored {},
}

/// The occurrence whose instance space a pass measures.
#[derive(Debug, Clone, Serialize)]
pub struct OccurrenceMeasures {
    pub(crate) occurrence: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) prototype: Option<String>,
    pub(crate) chain: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) transform: Option<[[f64; 4]; 3]>,
    pub(crate) bodies: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) descendants: Vec<DescendantMeasures>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) unavailable: Option<Unavailable<()>>,
}

accessors!(OccurrenceMeasures {
    /// The occurrence ID.
    occurrence: String;
    /// The local product definition the occurrence places, when it has one.
    prototype: Option<String>;
    /// The occurrences from the root to this one.
    chain: Vec<String>;
    /// The composed placement of the chain, as three rows of a 3×4 affine
    /// map, when the space places its bodies.
    transform: Option<[[f64; 4]; 3]>;
    /// The bodies of the subtree, sorted.
    bodies: Vec<String>;
    /// The occurrences below this one, breadth first.
    descendants: Vec<DescendantMeasures>;
    /// Why the pass emits no row in this space.
    unavailable: Option<Unavailable<()>>;
});

/// One occurrence below the occurrence whose instance space a pass
/// measures.
#[derive(Debug, Clone, Serialize)]
pub struct DescendantMeasures {
    pub(crate) occurrence: String,
    pub(crate) parent: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) prototype: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) transform: Option<[[f64; 4]; 3]>,
    pub(crate) bodies: Vec<String>,
}

accessors!(DescendantMeasures {
    /// The occurrence ID.
    occurrence: String;
    /// The containing occurrence.
    parent: String;
    /// The local product definition the occurrence places, when it has one.
    prototype: Option<String>;
    /// The composed placement from the root to this occurrence, as three
    /// rows of a 3×4 affine map, when the space places its bodies.
    transform: Option<[[f64; 4]; 3]>;
    /// The bodies its prototype lists.
    bodies: Vec<String>;
});

/// One surface row: the carrier of a surface, and the faces and bodies
/// that use it.
#[derive(Debug, Clone, Serialize)]
pub struct SurfaceMeasures {
    pub(crate) surface: String,
    pub(crate) placement: RowPlacement,
    pub(crate) carrier: CarrierMeasures,
    pub(crate) bodies: Vec<String>,
    pub(crate) faces: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) unresolved: Option<Unavailable<()>>,
    pub(crate) status: RowStatus,
}

accessors!(SurfaceMeasures {
    /// The surface ID.
    surface: String;
    /// The space the row is measured in.
    placement: RowPlacement;
    /// The carrier and its parameters.
    carrier: CarrierMeasures;
    /// The bodies of the faces that use the surface, sorted.
    bodies: Vec<String>;
    /// The faces that use the surface, sorted.
    faces: Vec<String>;
    /// Why the carrier has no placed parameters: its placement or its
    /// carrier chain does not resolve.
    unresolved: Option<Unavailable<()>>;
    /// Row status.
    status: RowStatus;
});

/// The composed placement of a carrier.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct PlacementReport {
    pub(crate) class: PlacementClass,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) scale: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) reflection: Option<bool>,
}

impl PlacementReport {
    /// The placement class.
    #[must_use]
    pub const fn class(&self) -> PlacementClass {
        self.class
    }
}

/// Kind-specific carrier parameters, in model space.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum CarrierParameters {
    /// A plane.
    Plane {
        /// Plane origin.
        origin: Quantity<Point>,
        /// Unit carrier normal `S_u × S_v`.
        normal: Quantity<Point>,
        /// First chart axis.
        u_axis: Quantity<Point>,
    },
    /// A circular cylinder.
    Cylinder {
        /// Radius.
        radius: Quantity<Scalar>,
        /// Diameter.
        diameter: Quantity<Scalar>,
        /// A point of the axis.
        axis_point: Quantity<Point>,
        /// Unit axis direction.
        axis_direction: Quantity<Point>,
        /// Unit reference direction of the chart.
        ref_direction: Quantity<Point>,
    },
    /// A cone.
    Cone {
        /// Canonical half angle in `[0, π/2)`.
        half_angle: Quantity<Scalar>,
        /// Twice the half angle.
        included_angle: Quantity<Scalar>,
        /// The apex, absent for a cone of zero half angle.
        #[serde(skip_serializing_if = "Option::is_none")]
        apex: Option<Quantity<Point>>,
        /// Unit axis direction toward growing radius.
        opening_direction: Quantity<Point>,
        /// Minor-to-major radius ratio, only when it is not one.
        #[serde(skip_serializing_if = "Option::is_none")]
        ratio: Option<Quantity<Scalar>>,
        /// Cross-section radius at the stored origin.
        reference_radius: Quantity<Scalar>,
        /// The stored origin.
        reference_point: Quantity<Point>,
    },
    /// A sphere.
    Sphere {
        /// Radius magnitude.
        radius: Quantity<Scalar>,
        /// Diameter.
        diameter: Quantity<Scalar>,
        /// Centre.
        center: Quantity<Point>,
        /// Unit axis direction: latitude grows toward it.
        axis_direction: Quantity<Point>,
        /// Unit reference direction: longitude 0 of the angular sweep.
        ref_direction: Quantity<Point>,
        /// Whether the carrier normal points away from the centre.
        normal_direction: NormalDirection,
    },
    /// A torus.
    Torus {
        /// Distance from the centre to the tube centre.
        major_radius: Quantity<Scalar>,
        /// Tube radius magnitude.
        minor_radius: Quantity<Scalar>,
        /// Centre.
        center: Quantity<Point>,
        /// Unit axis direction.
        axis_direction: Quantity<Point>,
        /// Ring, horn or spindle.
        form: TorusForm,
    },
    /// A NURBS carrier: descriptive facts, not quantities. A NURBS carrier
    /// has no radius.
    Nurbs {
        /// Degree in `u`.
        u_degree: u32,
        /// Degree in `v`.
        v_degree: u32,
        /// Pole count in `u`.
        u_poles: usize,
        /// Pole count in `v`.
        v_poles: usize,
        /// Whether the poles carry weights.
        rational: bool,
        /// Whether the carrier is periodic in `u`.
        u_periodic: bool,
        /// Whether the carrier is periodic in `v`.
        v_periodic: bool,
        /// The `u` knot domain.
        u_domain: [f64; 2],
        /// The `v` knot domain.
        v_domain: [f64; 2],
        /// Whether the oriented normal is opposite `S_u × S_v`.
        normal_reversed: bool,
    },
    /// A carrier with no kind-specific parameters in this build.
    None {},
}

/// Whether a sphere's carrier normal points away from its centre.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NormalDirection {
    /// Away from the centre.
    Outward,
    /// Toward the centre.
    Inward,
}

/// The form of a torus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TorusForm {
    /// The tube radius is below the major radius.
    Ring,
    /// The tube radius equals the major radius.
    Horn,
    /// The tube radius exceeds the major radius.
    Spindle,
}

/// The oriented normal a face states or a reading gives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NormalReading {
    /// The carrier normal `S_u × S_v`.
    Surface,
    /// The reversed carrier normal.
    Reversed,
    /// Loops disagree with each other.
    Mixed,
    /// No reading decides.
    NotDeterminable,
}

/// Computed loop roles against the face's stored classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Classification {
    /// The stored outer loop is the computed outer loop.
    Consistent,
    /// The stored outer loop is not the computed outer loop.
    Conflict,
    /// The face does not classify its loops, or no loop role is computed.
    Unspecified,
}

/// The orientation facts of a face.
#[derive(Debug, Clone, Serialize)]
pub struct FaceOrientation {
    pub(crate) declared_normal: NormalReading,
    pub(crate) loop_normal: NormalReading,
    pub(crate) witness: Option<Witness>,
    pub(crate) consistent: Option<bool>,
    pub(crate) classification: Classification,
}

accessors!(FaceOrientation {
    /// The oriented normal the face's sense states.
    declared_normal: NormalReading;
    /// The oriented normal the loop winding gives.
    loop_normal: NormalReading;
    /// The witness that confirms or contradicts a declared orientation.
    witness: Option<Witness>;
    /// Whether the readings agree; `None` when undetermined.
    consistent: Option<bool>;
    /// Computed loop roles against the stored classification.
    classification: Classification;
});

/// A witness of a declared-orientation face.
#[derive(Debug, Clone, Serialize)]
pub struct Witness {
    pub(crate) kind: WitnessKind,
    pub(crate) entity: String,
    pub(crate) agrees: bool,
}

accessors!(Witness {
    /// What decided the face's reading.
    kind: WitnessKind;
    /// The deciding edge, shell or face.
    entity: String;
    /// Whether the witness agrees with the declared orientation.
    agrees: bool;
});

/// The kind of a witness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WitnessKind {
    /// A tangent edge shared with an established face.
    TangentEdge,
    /// The sign of a closed shell's volume.
    ShellSign,
    /// One reading of the face has zero area.
    EmptyReading,
}

/// The region rule that decides a face's area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RegionRule {
    /// Loop roles from geometry and classification.
    Nesting,
    /// Two bounded regions; the declared orientation picks one.
    DeclaredOrientation,
    /// The face covers its whole carrier.
    WholeSurface,
    /// The carrier's domain is the outer boundary.
    DomainExterior,
}

/// The stored role of a loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeclaredRole {
    /// The face's classified outer loop.
    Outer,
    /// A classified inner loop.
    Inner,
    /// The face does not classify this loop.
    Unspecified,
}

/// The computed role of a loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LoopRole {
    /// The outer boundary.
    Outer,
    /// A hole.
    Inner,
    /// The lower loop of a band.
    BandLower,
    /// The upper loop of a band.
    BandUpper,
    /// A vertex loop.
    Vertex,
    /// A loop of a face whose region the domain or orientation decides.
    Domain,
}

/// The direction a loop runs in the parameter plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Winding {
    /// A contractible loop that encloses its inside on its left.
    Counterclockwise,
    /// A contractible loop that encloses its inside on its right.
    Clockwise,
    /// A loop that wraps a periodic direction forward.
    NonContractibleForward,
    /// A loop that wraps a periodic direction backward.
    NonContractibleBackward,
    /// A vertex loop.
    Point,
}

/// The listed coedge order of a ring against its traversal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ListedOrder {
    /// The listed order is a rotation of the traversal.
    Traversal,
    /// The listed order is a rotation of the reversed traversal.
    Reversed,
    /// Any other order.
    Other,
}

/// Where a coedge's parameter-space image comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UvSource {
    /// One pcurve use.
    Pcurve,
    /// A chain of pcurve uses.
    PcurveChain,
    /// The edge curve inverted onto the surface.
    InvertedEdgeCurve,
    /// The coedge's use-curve inverted onto the surface.
    InvertedUseCurve,
    /// A pole segment.
    Pole,
    /// A segment the resolver adds at a junction.
    Synthesized,
}

/// One coedge of a loop entry.
#[derive(Debug, Clone, Serialize)]
pub struct CoedgeMeasures {
    pub(crate) coedge: String,
    pub(crate) edge: String,
    pub(crate) seam: bool,
    pub(crate) uv_source: Option<UvSource>,
    pub(crate) length: Option<f64>,
}

accessors!(CoedgeMeasures {
    /// The coedge ID.
    coedge: String;
    /// The edge ID.
    edge: String;
    /// Whether another coedge of the edge belongs to the same face.
    seam: bool;
    /// Where the parameter-space image comes from.
    uv_source: Option<UvSource>;
    /// The edge length, when measured.
    length: Option<f64>;
});

/// One loop of a face row.
#[derive(Debug, Clone, Serialize)]
pub struct LoopMeasures {
    #[serde(rename = "loop")]
    pub(crate) loop_id: String,
    pub(crate) declared_role: DeclaredRole,
    pub(crate) role: Option<LoopRole>,
    pub(crate) winding: Option<Winding>,
    pub(crate) listed_order: Option<ListedOrder>,
    pub(crate) net_shift: Option<[i32; 2]>,
    pub(crate) max_gap: Option<f64>,
    pub(crate) length: Option<f64>,
    pub(crate) seam_length: Option<f64>,
    pub(crate) coedges: Vec<CoedgeMeasures>,
}

accessors!(LoopMeasures {
    /// The loop ID.
    loop_id: String;
    /// The stored role.
    declared_role: DeclaredRole;
    /// The computed role.
    role: Option<LoopRole>;
    /// The traversal direction in the parameter plane.
    winding: Option<Winding>;
    /// The listed coedge order against the traversal.
    listed_order: Option<ListedOrder>;
    /// Periods the loop wraps in `u` and `v`.
    net_shift: Option<[i32; 2]>;
    /// The largest accepted junction, closure or vertex gap, in millimetres.
    max_gap: Option<f64>;
    /// Boundary length without seams.
    length: Option<f64>;
    /// Seam length.
    seam_length: Option<f64>;
    /// Coedges in traversal order.
    coedges: Vec<CoedgeMeasures>;
});

/// The measured quantities of a face.
#[derive(Debug, Clone, Serialize)]
pub struct FaceQuantities {
    pub(crate) area: Quantity<Scalar>,
    pub(crate) boundary_length: Quantity<Scalar>,
    pub(crate) seam_length: Quantity<Scalar>,
    pub(crate) bbox: Quantity<BoxValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) axial_range: Option<Quantity<Range>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) axial_extent: Option<Quantity<Scalar>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) angular_sweep: Option<Quantity<Sweep>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) latitude_range: Option<Quantity<Range>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) radius_range: Option<Quantity<Range>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) u_range: Option<Quantity<Range>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) v_range: Option<Quantity<Range>>,
    pub(crate) region: Option<RegionRule>,
    pub(crate) loops: Vec<LoopMeasures>,
}

accessors!(FaceQuantities {
    /// Trimmed area.
    area: Quantity<Scalar>;
    /// Boundary length without seams.
    boundary_length: Quantity<Scalar>;
    /// Length of distinct seam edges.
    seam_length: Quantity<Scalar>;
    /// Axis-aligned box.
    bbox: Quantity<BoxValue>;
    /// Axial range of a cylinder or cone face.
    axial_range: Option<Quantity<Range>>;
    /// Axial extent of a cylinder or cone face.
    axial_extent: Option<Quantity<Scalar>>;
    /// Angular sweep about the axis.
    angular_sweep: Option<Quantity<Sweep>>;
    /// Latitude range of a sphere face.
    latitude_range: Option<Quantity<Range>>;
    /// Cross-section radius range of a cone face.
    radius_range: Option<Quantity<Range>>;
    /// Parameter range in `u`.
    u_range: Option<Quantity<Range>>;
    /// Parameter range in `v`.
    v_range: Option<Quantity<Range>>;
    /// The region rule, when one applies.
    region: Option<RegionRule>;
    /// Loops in face order.
    loops: Vec<LoopMeasures>;
});

/// One face row.
#[derive(Debug, Clone, Serialize)]
pub struct FaceMeasures {
    pub(crate) face: String,
    pub(crate) placement: RowPlacement,
    pub(crate) name: Option<String>,
    pub(crate) body: Option<String>,
    pub(crate) shell: String,
    pub(crate) sense: cadmpeg_ir::topology::Sense,
    pub(crate) carrier: CarrierMeasures,
    pub(crate) measures: FaceQuantities,
    pub(crate) orientation: FaceOrientation,
    pub(crate) caveats: Vec<Caveat>,
    pub(crate) status: RowStatus,
}

accessors!(FaceMeasures {
    /// The face ID.
    face: String;
    /// The space the row is measured in.
    placement: RowPlacement;
    /// The stored name.
    name: Option<String>;
    /// The owning body.
    body: Option<String>;
    /// The owning shell.
    shell: String;
    /// The stored sense.
    sense: cadmpeg_ir::topology::Sense;
    /// The carrier and its parameters.
    carrier: CarrierMeasures;
    /// The measured quantities.
    measures: FaceQuantities;
    /// Orientation facts.
    orientation: FaceOrientation;
    /// Face caveats.
    caveats: Vec<Caveat>;
    /// Row status.
    status: RowStatus;
});

/// Kind-specific edge curve parameters.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum EdgeParameters {
    /// A line.
    Line {
        /// Unit direction.
        direction: Quantity<Point>,
    },
    /// A circle.
    Circle {
        /// Radius.
        radius: Quantity<Scalar>,
        /// Centre.
        center: Quantity<Point>,
        /// Unit plane normal.
        normal: Quantity<Point>,
    },
    /// An ellipse.
    Ellipse {
        /// Major radius.
        major_radius: Quantity<Scalar>,
        /// Minor radius.
        minor_radius: Quantity<Scalar>,
        /// Centre.
        center: Quantity<Point>,
        /// Unit plane normal.
        normal: Quantity<Point>,
    },
    /// No kind-specific parameters.
    None {},
}

/// One edge row.
#[derive(Debug, Clone, Serialize)]
pub struct EdgeMeasures {
    pub(crate) edge: String,
    pub(crate) placement: RowPlacement,
    pub(crate) curve: Option<String>,
    pub(crate) kind: CurveKind,
    pub(crate) parameters: EdgeParameters,
    pub(crate) length: Quantity<Scalar>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) arc_angle: Option<Quantity<Scalar>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) parameter_span: Option<Quantity<Scalar>>,
    pub(crate) start: Quantity<Point>,
    pub(crate) end: Quantity<Point>,
    pub(crate) closed: bool,
    pub(crate) degenerate: bool,
    pub(crate) faces: Vec<String>,
    pub(crate) bbox: Quantity<BoxValue>,
    pub(crate) status: RowStatus,
}

accessors!(EdgeMeasures {
    /// The edge ID.
    edge: String;
    /// The space the row is measured in.
    placement: RowPlacement;
    /// The curve ID.
    curve: Option<String>;
    /// The curve kind.
    kind: CurveKind;
    /// Kind-specific parameters.
    parameters: EdgeParameters;
    /// Edge length.
    length: Quantity<Scalar>;
    /// Arc angle of a circular edge.
    arc_angle: Option<Quantity<Scalar>>;
    /// Parametric span of a conic edge that is not a circle.
    parameter_span: Option<Quantity<Scalar>>;
    /// Start point.
    start: Quantity<Point>;
    /// End point.
    end: Quantity<Point>;
    /// Whether the edge starts and ends at one vertex.
    closed: bool;
    /// Whether the edge's image is one point.
    degenerate: bool;
    /// Faces that use the edge.
    faces: Vec<String>;
    /// Axis-aligned box.
    bbox: Quantity<BoxValue>;
    /// Row status.
    status: RowStatus;
});

/// One shell of a body row.
#[derive(Debug, Clone, Serialize)]
pub struct ShellMeasures {
    pub(crate) shell: String,
    pub(crate) role: ShellRole,
    pub(crate) closed: bool,
    pub(crate) open_edges: Vec<String>,
    pub(crate) open_edge_count: usize,
    pub(crate) natural_edge_faces: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) volume: Option<Quantity<Scalar>>,
}

accessors!(ShellMeasures {
    /// The shell ID.
    shell: String;
    /// Outer or void.
    role: ShellRole;
    /// Whether every edge is used twice with opposite senses and no face
    /// reaches an uncovered natural domain edge.
    closed: bool;
    /// The first open edges.
    open_edges: Vec<String>;
    /// The number of open edges.
    open_edge_count: usize;
    /// Faces whose region reaches a natural domain edge.
    natural_edge_faces: Vec<String>;
    /// The shell's signed volume under the body's reading, when measured.
    volume: Option<Quantity<Scalar>>;
});

/// Closure fluxes of a body under its two orientation readings.
#[derive(Debug, Clone, Serialize)]
pub struct ClosureFlux {
    pub(crate) face_sense: Quantity<Scalar>,
    pub(crate) loop_winding: Quantity<Scalar>,
}

accessors!(ClosureFlux {
    /// `|Σ σ ∫ N|` with every face's sense.
    face_sense: Quantity<Scalar>;
    /// `|Σ σ ∫ N|` with every face's loop winding where it is determinable.
    loop_winding: Quantity<Scalar>;
});

/// The orientation facts of a body.
#[derive(Debug, Clone, Serialize)]
pub struct BodyOrientation {
    pub(crate) consistent: Option<bool>,
    #[serde(rename = "inconsistent_faces")]
    pub(crate) inconsistent: Vec<String>,
    #[serde(rename = "mixed_faces")]
    pub(crate) mixed: Vec<String>,
    #[serde(rename = "unconfirmed_faces")]
    pub(crate) unconfirmed: Vec<String>,
    #[serde(rename = "conflict_faces")]
    pub(crate) conflict: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) closure_flux: Option<ClosureFlux>,
}

accessors!(BodyOrientation {
    /// `true` when every face's readings agree, `false` when one disagrees,
    /// `null` otherwise.
    consistent: Option<bool>;
    /// Faces whose sense disagrees with their loop winding.
    inconsistent: Vec<String>;
    /// Faces whose loops disagree with each other.
    mixed: Vec<String>;
    /// Declared-orientation faces no witness confirms.
    unconfirmed: Vec<String>;
    /// Declared-orientation faces a witness contradicts.
    conflict: Vec<String>;
    /// Closure fluxes under both readings; absent for a body with no faces.
    closure_flux: Option<ClosureFlux>;
});

/// The role of a shell in its region.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ShellRole {
    /// The first shell of a region.
    Outer,
    /// A later shell of a region.
    Void,
}

/// Face caveats of a body, per code.
#[derive(Debug, Clone, Serialize)]
pub struct CaveatCount {
    pub(crate) code: CaveatCode,
    pub(crate) category: Category,
    pub(crate) faces: usize,
}

accessors!(CaveatCount {
    /// The caveat code.
    code: CaveatCode;
    /// The category of the code.
    category: Category;
    /// The number of member faces that carry the caveat.
    faces: usize;
});

/// One body row.
#[derive(Debug, Clone, Serialize)]
pub struct BodyMeasures {
    pub(crate) body: String,
    pub(crate) placement: RowPlacement,
    pub(crate) name: Option<String>,
    pub(crate) kind: cadmpeg_ir::topology::BodyKind,
    pub(crate) visible: Option<bool>,
    pub(crate) faces: usize,
    pub(crate) surface_kinds: BTreeMap<SurfaceKind, usize>,
    pub(crate) edges: usize,
    pub(crate) vertices: usize,
    pub(crate) shells: Vec<ShellMeasures>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) area: Option<Quantity<Scalar>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) volume: Option<Quantity<Scalar>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) centroid: Option<Quantity<Point>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) second_moments: Option<Quantity<Tensor>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) principal_moments: Option<[Quantity<Scalar>; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) principal_axes: Option<[Quantity<Point>; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) mass: Option<Quantity<Scalar>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) inertia: Option<Quantity<Tensor>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) area_centroid: Option<Quantity<Point>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) length: Option<Quantity<Scalar>>,
    pub(crate) bbox: Quantity<BoxValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) vertex_box: Option<Quantity<BoxValue>>,
    pub(crate) orientation: BodyOrientation,
    pub(crate) caveats: Vec<CaveatCount>,
    pub(crate) status: RowStatus,
}

accessors!(BodyMeasures {
    /// The body ID.
    body: String;
    /// The space the row is measured in.
    placement: RowPlacement;
    /// The stored name.
    name: Option<String>;
    /// The stored kind.
    kind: cadmpeg_ir::topology::BodyKind;
    /// The stored visibility.
    visible: Option<bool>;
    /// The number of faces.
    faces: usize;
    /// Faces per canonical kind. Informational.
    surface_kinds: BTreeMap<SurfaceKind, usize>;
    /// The number of edges.
    edges: usize;
    /// The number of vertices.
    vertices: usize;
    /// Shells in region order.
    shells: Vec<ShellMeasures>;
    /// Sum of face areas; absent for a body with no faces.
    area: Option<Quantity<Scalar>>;
    /// Enclosed volume; absent for a body with no faces.
    volume: Option<Quantity<Scalar>>;
    /// Volume centroid; absent for a body with no faces.
    centroid: Option<Quantity<Point>>;
    /// Second moments of volume about the centroid, `∫ (x − c)(x − c)ᵀ
    /// dV`; absent for a body with no faces.
    second_moments: Option<Quantity<Tensor>>;
    /// The three eigenvalues of the second moments, smallest first.
    principal_moments: Option<[Quantity<Scalar>; 3]>;
    /// The unit eigenvector of each principal moment, in their order; an
    /// axis whose moment another moment equals within their separation is
    /// unavailable.
    principal_axes: Option<[Quantity<Point>; 3]>;
    /// Mass at the given density; absent without one.
    mass: Option<Quantity<Scalar>>;
    /// Inertia tensor about the centroid at the given density; absent
    /// without one.
    inertia: Option<Quantity<Tensor>>;
    /// Area-weighted centroid of a body with faces and no closed shell.
    area_centroid: Option<Quantity<Point>>;
    /// Total edge length of a body with edges and no faces.
    length: Option<Quantity<Scalar>>;
    /// Union of member boxes.
    bbox: Quantity<BoxValue>;
    /// Box of the vertex points; absent for a body with no vertex.
    vertex_box: Option<Quantity<BoxValue>>;
    /// Orientation facts.
    orientation: BodyOrientation;
    /// Face caveats per code.
    caveats: Vec<CaveatCount>;
    /// Row status.
    status: RowStatus;
});

/// The work the pass used.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct WorkReport {
    pub(crate) allowance: usize,
    pub(crate) used: usize,
    pub(crate) exhausted: bool,
}

impl WorkReport {
    /// The pass allowance in work units.
    #[must_use]
    pub const fn allowance(&self) -> usize {
        self.allowance
    }

    /// Work units used.
    #[must_use]
    pub const fn used(&self) -> usize {
        self.used
    }

    /// Whether an item ran out because the pass was empty.
    #[must_use]
    pub const fn exhausted(&self) -> bool {
        self.exhausted
    }
}

/// Faces named by the document orientation lists.
#[derive(Debug, Clone, Default, Serialize)]
pub struct DocumentOrientation {
    #[serde(rename = "inconsistent_faces")]
    pub(crate) inconsistent: Vec<String>,
    #[serde(rename = "mixed_faces")]
    pub(crate) mixed: Vec<String>,
    #[serde(rename = "unconfirmed_faces")]
    pub(crate) unconfirmed: Vec<String>,
    #[serde(rename = "conflict_faces")]
    pub(crate) conflict: Vec<String>,
    #[serde(rename = "ring_order_faces")]
    pub(crate) ring_order: Vec<String>,
}

accessors!(DocumentOrientation {
    /// Faces whose sense disagrees with their loop winding.
    inconsistent: Vec<String>;
    /// Faces whose loops disagree with each other.
    mixed: Vec<String>;
    /// Declared-orientation faces no witness confirms.
    unconfirmed: Vec<String>;
    /// Declared-orientation faces a witness contradicts.
    conflict: Vec<String>;
    /// Faces with a ring listed out of traversal order.
    ring_order: Vec<String>;
});

/// Document totals and counts.
#[derive(Debug, Clone, Serialize)]
pub struct DocumentMeasures {
    pub(crate) bodies: usize,
    pub(crate) faces: usize,
    pub(crate) edges: usize,
    pub(crate) surfaces: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) area: Option<Quantity<Scalar>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) closed_volume: Option<Quantity<Scalar>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) bbox: Option<Quantity<BoxValue>>,
    pub(crate) orientation: DocumentOrientation,
    pub(crate) unavailable: BTreeMap<Category, BTreeMap<String, usize>>,
    pub(crate) work: WorkReport,
}

accessors!(DocumentMeasures {
    /// Bodies in scope.
    bodies: usize;
    /// Faces in scope.
    faces: usize;
    /// Edges in scope.
    edges: usize;
    /// Surface rows.
    surfaces: usize;
    /// Total area of the body rows; absent without body rows. Unavailable
    /// with the occurrence's reason when the occurrence space has no rows.
    area: Option<Quantity<Scalar>>;
    /// Total volume of the body rows with faces; absent without them.
    /// Unavailable with the occurrence's reason when the occurrence space
    /// has no rows.
    closed_volume: Option<Quantity<Scalar>>;
    /// Union box of the body rows; absent without body rows. Unavailable
    /// with the occurrence's reason when the occurrence space has no rows.
    bbox: Option<Quantity<BoxValue>>;
    /// Orientation lists.
    orientation: DocumentOrientation;
    /// Unavailable values in the emitted rows and the occurrence block, by
    /// category and code.
    unavailable: BTreeMap<Category, BTreeMap<String, usize>>;
    /// The work the pass used.
    work: WorkReport;
});

/// Collects the reason codes of a row's unavailable values.
#[derive(Default)]
struct Codes(std::collections::BTreeSet<&'static str>);

impl QuantityVisitor for Codes {
    fn visit<T>(&mut self, quantity: &Quantity<T>) {
        if let Some(unavailable) = quantity.unavailable() {
            self.0.insert(unavailable.reason().code());
        }
    }
}

impl FaceMeasures {
    /// The distinct reason codes of the row's unavailable values, sorted.
    #[must_use]
    pub fn unavailable_codes(&self) -> Vec<&'static str> {
        let mut codes = Codes::default();
        self.visit(&mut codes);
        codes.0.into_iter().collect()
    }
}

impl EdgeMeasures {
    /// The distinct reason codes of the row's unavailable values, sorted.
    #[must_use]
    pub fn unavailable_codes(&self) -> Vec<&'static str> {
        let mut codes = Codes::default();
        self.visit(&mut codes);
        codes.0.into_iter().collect()
    }
}

impl SurfaceMeasures {
    /// The distinct reason codes of the row's unavailable values, sorted.
    #[must_use]
    pub fn unavailable_codes(&self) -> Vec<&'static str> {
        let mut codes = Codes::default();
        self.visit(&mut codes);
        codes.0.into_iter().collect()
    }

    pub(crate) fn visit(&self, visitor: &mut impl QuantityVisitor) {
        self.carrier.parameters.visit(visitor);
        if let Some(unresolved) = &self.unresolved {
            visitor.visit(&Quantity::Unavailable(unresolved.clone()));
        }
    }
}

impl BodyMeasures {
    /// The distinct reason codes of the row's unavailable values, sorted.
    #[must_use]
    pub fn unavailable_codes(&self) -> Vec<&'static str> {
        let mut codes = Codes::default();
        self.visit(&mut codes);
        codes.0.into_iter().collect()
    }
}

impl CarrierParameters {
    pub(crate) fn visit(&self, visitor: &mut impl QuantityVisitor) {
        match self {
            Self::Plane {
                origin,
                normal,
                u_axis,
            } => {
                visitor.visit(origin);
                visitor.visit(normal);
                visitor.visit(u_axis);
            }
            Self::Cylinder {
                radius,
                diameter,
                axis_point,
                axis_direction,
                ref_direction,
            } => {
                visitor.visit(radius);
                visitor.visit(diameter);
                visitor.visit(axis_point);
                visitor.visit(axis_direction);
                visitor.visit(ref_direction);
            }
            Self::Cone {
                half_angle,
                included_angle,
                apex,
                opening_direction,
                ratio,
                reference_radius,
                reference_point,
            } => {
                visitor.visit(half_angle);
                visitor.visit(included_angle);
                visitor.visit_option(apex.as_ref());
                visitor.visit(opening_direction);
                visitor.visit_option(ratio.as_ref());
                visitor.visit(reference_radius);
                visitor.visit(reference_point);
            }
            Self::Sphere {
                radius,
                diameter,
                center,
                axis_direction,
                ref_direction,
                ..
            } => {
                visitor.visit(radius);
                visitor.visit(diameter);
                visitor.visit(center);
                visitor.visit(axis_direction);
                visitor.visit(ref_direction);
            }
            Self::Torus {
                major_radius,
                minor_radius,
                center,
                axis_direction,
                ..
            } => {
                visitor.visit(major_radius);
                visitor.visit(minor_radius);
                visitor.visit(center);
                visitor.visit(axis_direction);
            }
            Self::Nurbs { .. } | Self::None {} => {}
        }
    }
}

impl FaceQuantities {
    pub(crate) fn visit(&self, visitor: &mut impl QuantityVisitor) {
        visitor.visit(&self.area);
        visitor.visit(&self.boundary_length);
        visitor.visit(&self.seam_length);
        visitor.visit(&self.bbox);
        visitor.visit_option(self.axial_range.as_ref());
        visitor.visit_option(self.axial_extent.as_ref());
        visitor.visit_option(self.angular_sweep.as_ref());
        visitor.visit_option(self.latitude_range.as_ref());
        visitor.visit_option(self.radius_range.as_ref());
        visitor.visit_option(self.u_range.as_ref());
        visitor.visit_option(self.v_range.as_ref());
    }
}

impl FaceMeasures {
    pub(crate) fn visit(&self, visitor: &mut impl QuantityVisitor) {
        self.carrier.parameters.visit(visitor);
        self.measures.visit(visitor);
    }
}

impl EdgeParameters {
    pub(crate) fn visit(&self, visitor: &mut impl QuantityVisitor) {
        match self {
            Self::Line { direction } => visitor.visit(direction),
            Self::Circle {
                radius,
                center,
                normal,
            } => {
                visitor.visit(radius);
                visitor.visit(center);
                visitor.visit(normal);
            }
            Self::Ellipse {
                major_radius,
                minor_radius,
                center,
                normal,
            } => {
                visitor.visit(major_radius);
                visitor.visit(minor_radius);
                visitor.visit(center);
                visitor.visit(normal);
            }
            Self::None {} => {}
        }
    }
}

impl EdgeMeasures {
    pub(crate) fn visit(&self, visitor: &mut impl QuantityVisitor) {
        self.parameters.visit(visitor);
        visitor.visit(&self.length);
        visitor.visit_option(self.arc_angle.as_ref());
        visitor.visit_option(self.parameter_span.as_ref());
        visitor.visit(&self.start);
        visitor.visit(&self.end);
        visitor.visit(&self.bbox);
    }
}

impl BodyMeasures {
    pub(crate) fn visit(&self, visitor: &mut impl QuantityVisitor) {
        visitor.visit_option(self.area.as_ref());
        visitor.visit_option(self.volume.as_ref());
        visitor.visit_option(self.centroid.as_ref());
        visitor.visit_option(self.second_moments.as_ref());
        for moment in self.principal_moments.iter().flatten() {
            visitor.visit(moment);
        }
        for axis in self.principal_axes.iter().flatten() {
            visitor.visit(axis);
        }
        visitor.visit_option(self.mass.as_ref());
        visitor.visit_option(self.inertia.as_ref());
        visitor.visit_option(self.area_centroid.as_ref());
        visitor.visit_option(self.length.as_ref());
        visitor.visit(&self.bbox);
        visitor.visit_option(self.vertex_box.as_ref());
    }
}

// SPDX-License-Identifier: Apache-2.0
#![cfg_attr(test, allow(clippy::unwrap_used))]
//! Display tessellation of cadmpeg B-rep faces.
//!
//! [`Tessellator`] reads a [`cadmpeg_ir::CadIr`] and triangulates one face
//! at a time. Each face's loops become polylines in the parameter plane of
//! its surface: from the coedge pcurves when the model has them, otherwise
//! by inverting samples of the edge curves onto the surface. Edge samples are
//! shared by every face that uses the edge, so adjacent faces meet at the
//! same points. The trimmed parameter domain is triangulated and refined
//! until the chordal deviation and normal-angle limits of [`Options`] hold.
//!
//! The result also carries each coedge's directed parameter-space polyline,
//! a derived pcurve, and the largest distance between a boundary point and
//! the surface.
//!
//! ```
//! use cadmpeg_tessellate::{Options, Tessellator};
//!
//! let cube = cadmpeg_ir::examples::unit_cube().expect("fixture");
//! let mut tessellator = Tessellator::new(&cube, Options::default());
//! let face = &cube.model.faces[0].id;
//! let mesh = tessellator.face(face).expect("planar face");
//! assert_eq!(mesh.triangles.len(), 2);
//! ```

mod boundary;
mod domain;
mod face;
mod plane;
mod refine;
mod surface;
mod triangulate;

pub use face::{CoedgeTrace, FaceMesh, Refusal};

use cadmpeg_ir::ids::FaceId;
use cadmpeg_ir::index::ModelIndex;
use cadmpeg_ir::CadIr;

/// Accuracy and size limits of a tessellation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Options {
    /// Largest distance, in model length units, between a triangle and the
    /// surface, and between a boundary segment and its edge curve.
    pub chord: f64,
    /// Largest angle, in radians, between the surface normals at two
    /// vertices of one triangle, and between two consecutive boundary
    /// segments.
    pub angle: f64,
    /// Largest distance, in model length units, between a derived pcurve
    /// mapped onto the surface and its edge curve. `None` derives the pcurve
    /// at the display accuracy.
    pub pcurve_tolerance: Option<f64>,
    /// Largest vertex count of one face mesh.
    pub max_vertices: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            chord: 0.01,
            angle: 0.25,
            pcurve_tolerance: None,
            max_vertices: 200_000,
        }
    }
}

/// Tessellates the faces of one model.
pub struct Tessellator<'ir> {
    index: ModelIndex<'ir>,
    options: Options,
    edges: boundary::EdgeCache,
}

impl<'ir> Tessellator<'ir> {
    /// A tessellator of `ir`.
    #[must_use]
    pub fn new(ir: &'ir CadIr, options: Options) -> Self {
        Self {
            index: ModelIndex::new_model_only(ir),
            options,
            edges: boundary::EdgeCache::default(),
        }
    }

    /// The options of this tessellator.
    #[must_use]
    pub const fn options(&self) -> Options {
        self.options
    }

    /// Replaces the options for the faces tessellated next. Edge samples
    /// already taken are kept, so faces that share an edge share its
    /// samples.
    pub fn set_options(&mut self, options: Options) {
        self.options = options;
    }

    /// The mesh of face `face`.
    ///
    /// # Errors
    ///
    /// Returns the reason when the face's boundary or surface cannot be
    /// evaluated, or its domain cannot be triangulated.
    pub fn face(&mut self, face: &FaceId) -> Result<FaceMesh, Refusal> {
        face::tessellate(self, face)
    }
}

#[cfg(test)]
mod integration_tests;

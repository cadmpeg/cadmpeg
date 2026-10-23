// SPDX-License-Identifier: Apache-2.0
//! Surface rows: the carrier of one surface, with no bounded measure.
//!
//! A surface row is measured under the placement of the bodies whose faces
//! use the surface. Those bodies must agree on it: a surface used by faces
//! of bodies with different placements has no placed parameters. A surface
//! that no face names is measured as stored, in the stored space: a face
//! that reaches it only through a carrier chain does not place it. A carrier
//! with no chart has no parameters, and its reason is the row's
//! `unresolved` value.

use std::collections::BTreeSet;

use cadmpeg_core::decode::ResourceLimit;
use cadmpeg_ir::geometry::Surface;
use cadmpeg_ir::source_fidelity::SourceFidelity;
use cadmpeg_ir::transform::Transform;

use crate::canonical::placement::Placement;
use crate::canonical::{Carrier, Sources};
use crate::failure::{settle, Missing, Outcome};
use crate::owners::Owners;
use crate::reason::UnavailableReason;
use crate::report::{
    CarrierMeasures, CarrierParameters, PlacementClass, PlacementReport, StatusTally,
    SurfaceMeasures,
};
use crate::select::SurfaceKind;

/// What a surface row reads.
pub(crate) struct SurfaceInput<'a, 'ir> {
    pub(crate) owners: &'a Owners<'ir>,
    pub(crate) fidelity: Option<&'a SourceFidelity>,
    /// The faces of the document, in ID order.
    pub(crate) faces: &'ir [cadmpeg_ir::topology::Face],
    /// The bodies of the occurrence space; absent in model space.
    pub(crate) scope: Option<&'a BTreeSet<&'ir str>>,
    /// The composed transform of each body, absent for the identity.
    pub(crate) transform: &'a dyn Fn(&str) -> Option<Transform>,
    /// The placement of each body.
    pub(crate) placement: &'a dyn Fn(Option<&str>) -> Outcome<Placement>,
}

impl<'ir> SurfaceInput<'_, 'ir> {
    /// The row of `surface`.
    pub(crate) fn row(&self, surface: &'ir Surface) -> Result<SurfaceMeasures, ResourceLimit> {
        let id = surface.id.as_str();
        let in_scope = |face: &cadmpeg_ir::topology::Face| match self.scope {
            None => true,
            Some(scope) => self
                .owners
                .face_body(face)
                .is_some_and(|body| scope.contains(body)),
        };
        let faces: Vec<&'ir cadmpeg_ir::topology::Face> = self
            .faces
            .iter()
            .filter(|face| face.surface == surface.id && in_scope(face))
            .collect();
        let bodies: BTreeSet<&'ir str> = faces
            .iter()
            .filter_map(|face| self.owners.face_body(face))
            .collect();
        let (placement, placed_body) = self.shared_placement(id, &bodies);
        let resolved = match &placement {
            Ok(placement) => settle(
                Carrier::resolve_in(self.owners, surface, *placement)
                    .map_err(|failure| failure.at(id)),
            )?,
            Err(missing) => Err(missing.clone()),
        };
        let (carrier, unresolved) = match resolved {
            Ok(carrier) => {
                let unresolved = settle(carrier.chart())?
                    .err()
                    .map(|missing| missing.unavailable::<()>());
                (
                    carrier.measures(Sources {
                        fidelity: self.fidelity,
                        surface: carrier.id(),
                        depth: carrier.depth,
                        body: placed_body,
                    }),
                    unresolved,
                )
            }
            Err(missing) => (self.unplaced(surface)?, Some(missing.unavailable::<()>())),
        };
        let placement = if faces.is_empty() {
            crate::report::RowPlacement::Stored {}
        } else {
            crate::report::RowPlacement::Model {}
        };
        let mut row = SurfaceMeasures {
            surface: id.to_owned(),
            placement,
            carrier,
            bodies: bodies.iter().map(|body| (*body).to_owned()).collect(),
            faces: faces
                .iter()
                .map(|face| face.id.as_str().to_owned())
                .collect(),
            unresolved,
            status: crate::report::RowStatus::Measured,
        };
        let mut tally = StatusTally::default();
        row.visit(&mut tally);
        row.status = tally.status();
        Ok(row)
    }

    /// The placement the bodies of the surface's faces share, and the body
    /// whose transform it is.
    fn shared_placement(
        &self,
        id: &str,
        bodies: &BTreeSet<&'ir str>,
    ) -> (Outcome<Placement>, Option<&'ir str>) {
        let transform = self.transform;
        let mut placed: Option<(Option<Transform>, &'ir str)> = None;
        for body in bodies {
            let own = transform(body);
            match placed {
                None => placed = Some((own, body)),
                Some((shared, _)) if shared == own => {}
                Some(_) => {
                    return (
                        Err(Missing {
                            reason: UnavailableReason::PlacementCarrierShared,
                            entity: Some(id.to_owned()),
                            message: "the surface is used by faces of bodies with different \
                                      placements"
                                .to_owned(),
                        }),
                        None,
                    )
                }
            }
        }
        match placed {
            None => (Ok(Placement::identity()), None),
            Some((shared, body)) => (
                (self.placement)(Some(body)),
                shared.is_some().then_some(body),
            ),
        }
    }

    /// The carrier row of a surface whose placement does not resolve: its
    /// kind and chain as stored, and no parameters.
    fn unplaced(&self, surface: &'ir Surface) -> Result<CarrierMeasures, ResourceLimit> {
        let stored = settle(Carrier::resolve_in(
            self.owners,
            surface,
            Placement::identity(),
        ))?;
        Ok(match stored {
            Ok(carrier) => CarrierMeasures {
                surface: surface.id.as_str().to_owned(),
                kind: carrier.kind,
                chain: carrier.chain.clone(),
                analytic: carrier.kind.analytic(),
                placement: PlacementReport {
                    class: PlacementClass::Identity,
                    scale: None,
                    reflection: None,
                },
                parameters: CarrierParameters::None {},
            },
            Err(_) => CarrierMeasures {
                surface: surface.id.as_str().to_owned(),
                kind: SurfaceKind::Unknown,
                chain: Vec::new(),
                analytic: false,
                placement: PlacementReport {
                    class: PlacementClass::Identity,
                    scale: None,
                    reflection: None,
                },
                parameters: CarrierParameters::None {},
            },
        })
    }
}

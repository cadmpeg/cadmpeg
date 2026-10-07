// SPDX-License-Identifier: Apache-2.0
//! Attaching procedural constructions to the carriers that own them.
//!
//! One attachment refuses a construction whose identity is already stored, a
//! construction another carrier already names, a missing owner, and an owner
//! already owned by another construction. A single attachment scans the
//! arenas. A batch indexes them once and keeps the index current as it
//! attaches, so each attachment is a keyed lookup; it holds the model
//! exclusively for the whole batch, so nothing else can change the arenas
//! under the index. Both make the same decisions in the same order.

use std::borrow::Borrow;
use std::fmt;

use super::admission::{IdentityPositions, ModelAdmission};
use super::{Model, ProceduralCarrierError};
use crate::geometry::{
    Curve, CurveGeometry, ProceduralCurve, ProceduralSurface, Surface, SurfaceGeometry,
};
use crate::ids::{CurveId, SurfaceId};

/// The owner state an attachment depends on.
pub(super) enum OwnerGeometry<'a> {
    /// Solved geometry, which the attachment turns procedural.
    Solved,
    /// Procedural geometry naming a construction, with or without a cache.
    Procedural { construction: &'a str, cached: bool },
}

/// One carrier family: curves or surfaces.
pub(super) trait CarrierKind {
    type Carrier;
    type Owner: fmt::Display + ?Sized;
    type Procedural;
    type Construction;

    const NOUN: &'static str;
    const SCAN_CONSTRUCTIONS: &'static str;
    const COMPARE_CONSTRUCTIONS: &'static str;
    const SCAN_CARRIERS: &'static str;
    const COMPARE_OWNERS: &'static str;
    const REFUSAL: &'static str;
    const STORE: &'static str;
    const OWNER_IDENTITY: &'static str;
    const INDEX: &'static str;

    fn carriers(model: &Model) -> &[Self::Carrier];
    fn carriers_mut(model: &mut Model) -> &mut [Self::Carrier];
    fn procedurals(model: &mut Model) -> &mut Vec<Self::Procedural>;
    fn carrier_id(carrier: &Self::Carrier) -> &str;
    fn owner_text(owner: &Self::Owner) -> &str;
    fn carrier_construction(carrier: &Self::Carrier) -> Option<&str>;
    fn owner_geometry(carrier: &Self::Carrier) -> OwnerGeometry<'_>;
    fn procedural_id(procedural: &Self::Procedural) -> &str;
    fn has_cache_tolerance(procedural: &Self::Procedural) -> bool;
    /// Copy the construction identity a solved owner will name.
    fn construction<A: ModelAdmission>(
        admission: &A,
        procedural: &Self::Procedural,
    ) -> Result<Self::Construction, A::Error>;
    /// Make a solved owner name `construction`, keeping its solved geometry
    /// as the cache.
    fn own(carrier: &mut Self::Carrier, construction: Self::Construction);
}

pub(super) struct CurveKind;
pub(super) struct SurfaceKind;

impl CarrierKind for CurveKind {
    type Carrier = Curve;
    type Owner = CurveId;
    type Procedural = ProceduralCurve;
    type Construction = crate::ids::ProceduralCurveId;

    const NOUN: &'static str = "curve";
    const SCAN_CONSTRUCTIONS: &'static str = "scan procedural curve constructions";
    const COMPARE_CONSTRUCTIONS: &'static str = "compare procedural curve constructions";
    const SCAN_CARRIERS: &'static str = "scan procedural curve carriers";
    const COMPARE_OWNERS: &'static str = "compare procedural curve owners";
    const REFUSAL: &'static str = "procedural curve refusal";
    const STORE: &'static str = "store procedural curve constructions";
    const OWNER_IDENTITY: &'static str = "ir_procedural_curve_construction_id";
    const INDEX: &'static str = "index procedural curve carriers";

    fn carriers(model: &Model) -> &[Curve] {
        &model.curves
    }
    fn carriers_mut(model: &mut Model) -> &mut [Curve] {
        &mut model.curves
    }
    fn procedurals(model: &mut Model) -> &mut Vec<ProceduralCurve> {
        &mut model.procedural_curves
    }
    fn carrier_id(carrier: &Curve) -> &str {
        carrier.id.as_str()
    }
    fn owner_text(owner: &CurveId) -> &str {
        owner.as_str()
    }
    fn carrier_construction(carrier: &Curve) -> Option<&str> {
        carrier
            .geometry
            .procedural_construction()
            .map(crate::ids::ProceduralCurveId::as_str)
    }
    fn owner_geometry(carrier: &Curve) -> OwnerGeometry<'_> {
        match &carrier.geometry {
            CurveGeometry::Solved(_) => OwnerGeometry::Solved,
            CurveGeometry::Procedural {
                construction,
                cache,
            } => OwnerGeometry::Procedural {
                construction: construction.as_str(),
                cached: cache.is_some(),
            },
        }
    }
    fn procedural_id(procedural: &ProceduralCurve) -> &str {
        procedural.id.as_str()
    }
    fn has_cache_tolerance(procedural: &ProceduralCurve) -> bool {
        procedural.cache_fit_tolerance().is_some()
    }
    fn construction<A: ModelAdmission>(
        admission: &A,
        procedural: &ProceduralCurve,
    ) -> Result<crate::ids::ProceduralCurveId, A::Error> {
        admission.curve_id(&procedural.id, Self::OWNER_IDENTITY)
    }
    fn own(carrier: &mut Curve, construction: crate::ids::ProceduralCurveId) {
        let previous = std::mem::replace(
            &mut carrier.geometry,
            CurveGeometry::Procedural {
                construction,
                cache: None,
            },
        );
        if let (CurveGeometry::Solved(geometry), CurveGeometry::Procedural { cache, .. }) =
            (previous, &mut carrier.geometry)
        {
            *cache = Some(geometry);
        }
    }
}

impl CarrierKind for SurfaceKind {
    type Carrier = Surface;
    type Owner = SurfaceId;
    type Procedural = ProceduralSurface;
    type Construction = crate::ids::ProceduralSurfaceId;

    const NOUN: &'static str = "surface";
    const SCAN_CONSTRUCTIONS: &'static str = "scan procedural surface constructions";
    const COMPARE_CONSTRUCTIONS: &'static str = "compare procedural surface constructions";
    const SCAN_CARRIERS: &'static str = "scan procedural surface carriers";
    const COMPARE_OWNERS: &'static str = "compare procedural surface owners";
    const REFUSAL: &'static str = "procedural surface refusal";
    const STORE: &'static str = "store procedural surface constructions";
    const OWNER_IDENTITY: &'static str = "procedural surface owner identity";
    const INDEX: &'static str = "index procedural surface carriers";

    fn carriers(model: &Model) -> &[Surface] {
        &model.surfaces
    }
    fn carriers_mut(model: &mut Model) -> &mut [Surface] {
        &mut model.surfaces
    }
    fn procedurals(model: &mut Model) -> &mut Vec<ProceduralSurface> {
        &mut model.procedural_surfaces
    }
    fn carrier_id(carrier: &Surface) -> &str {
        carrier.id.as_str()
    }
    fn owner_text(owner: &SurfaceId) -> &str {
        owner.as_str()
    }
    fn carrier_construction(carrier: &Surface) -> Option<&str> {
        carrier
            .geometry
            .procedural_construction()
            .map(crate::ids::ProceduralSurfaceId::as_str)
    }
    fn owner_geometry(carrier: &Surface) -> OwnerGeometry<'_> {
        match &carrier.geometry {
            SurfaceGeometry::Solved(_) => OwnerGeometry::Solved,
            SurfaceGeometry::Procedural {
                construction,
                cache,
            } => OwnerGeometry::Procedural {
                construction: construction.as_str(),
                cached: cache.is_some(),
            },
        }
    }
    fn procedural_id(procedural: &ProceduralSurface) -> &str {
        procedural.id.as_str()
    }
    fn has_cache_tolerance(procedural: &ProceduralSurface) -> bool {
        procedural.cache_fit_tolerance().is_some()
    }
    fn construction<A: ModelAdmission>(
        admission: &A,
        procedural: &ProceduralSurface,
    ) -> Result<crate::ids::ProceduralSurfaceId, A::Error> {
        admission.surface_id(&procedural.id, Self::OWNER_IDENTITY)
    }
    fn own(carrier: &mut Surface, construction: crate::ids::ProceduralSurfaceId) {
        let previous = std::mem::replace(
            &mut carrier.geometry,
            SurfaceGeometry::Procedural {
                construction,
                cache: None,
            },
        );
        if let (SurfaceGeometry::Solved(geometry), SurfaceGeometry::Procedural { cache, .. }) =
            (previous, &mut carrier.geometry)
        {
            *cache = Some(geometry);
        }
    }
}

fn refuse<K: CarrierKind, A: ModelAdmission, T>(
    admission: &A,
    message: fmt::Arguments<'_>,
) -> Result<Result<T, ProceduralCarrierError>, A::Error> {
    Ok(Err(ProceduralCarrierError::new(
        admission.text(message, K::REFUSAL)?,
    )))
}

/// How an attachment ended once its owner is known.
enum Attached {
    /// The owner was solved and now names the construction.
    Owned,
    /// The owner already named the construction directly.
    Direct,
}

/// Finish an attachment to the carrier at `owner`: refuse an owner owned by
/// another construction, then store the construction.
fn attach_at<K: CarrierKind, A: ModelAdmission>(
    model: &mut Model,
    admission: &A,
    owner: usize,
    owner_id: &K::Owner,
    procedural: K::Procedural,
) -> Result<Result<Attached, ProceduralCarrierError>, A::Error> {
    let id = K::procedural_id(&procedural);
    let attached = match K::owner_geometry(&K::carriers(model)[owner]) {
        OwnerGeometry::Procedural {
            construction,
            cached: false,
        } if admission.equal(construction, id, K::COMPARE_CONSTRUCTIONS)? => {
            if K::has_cache_tolerance(&procedural) {
                return refuse::<K, A, _>(
                    admission,
                    format_args!(
                        "direct procedural {} {owner_id} cannot carry a solved-cache tolerance",
                        K::NOUN
                    ),
                )
                .map(|refused| refused.map(|()| Attached::Direct));
            }
            admission.reserve(K::procedurals(model), 1, K::STORE)?;
            Attached::Direct
        }
        OwnerGeometry::Procedural { construction, .. } => {
            return refuse::<K, A, _>(
                admission,
                format_args!(
                    "{} {owner_id} is already owned by procedural construction {construction}",
                    K::NOUN
                ),
            );
        }
        OwnerGeometry::Solved => {
            let construction = K::construction(admission, &procedural)?;
            admission.reserve(K::procedurals(model), 1, K::STORE)?;
            K::own(&mut K::carriers_mut(model)[owner], construction);
            Attached::Owned
        }
    };
    K::procedurals(model).push(procedural);
    Ok(Ok(attached))
}

/// Attach one construction by scanning the arenas.
pub(super) fn attach_scanning<K: CarrierKind, A: ModelAdmission>(
    model: &mut Model,
    admission: &A,
    owner: &K::Owner,
    procedural: K::Procedural,
) -> Result<Result<(), ProceduralCarrierError>, A::Error> {
    let id = K::procedural_id(&procedural);
    let constructions = K::procedurals(model);
    for existing in constructions.iter() {
        admission.work(1, K::SCAN_CONSTRUCTIONS)?;
        if admission.equal(K::procedural_id(existing), id, K::COMPARE_CONSTRUCTIONS)? {
            return refuse::<K, A, _>(
                admission,
                format_args!("procedural {} construction {id} already exists", K::NOUN),
            );
        }
    }
    let mut owner_index = None;
    for (index, carrier) in K::carriers(model).iter().enumerate() {
        admission.work(1, K::SCAN_CARRIERS)?;
        let is_owner = admission.equal(
            K::carrier_id(carrier),
            K::owner_text(owner),
            K::COMPARE_OWNERS,
        )?;
        if owner_index.is_none() && is_owner {
            owner_index = Some(index);
        }
        if !is_owner
            && match K::carrier_construction(carrier) {
                Some(construction) => {
                    admission.equal(construction, id, K::COMPARE_CONSTRUCTIONS)?
                }
                None => false,
            }
        {
            return refuse::<K, A, _>(
                admission,
                format_args!(
                    "procedural {noun} construction {id} already owns {noun} {}",
                    K::carrier_id(carrier),
                    noun = K::NOUN
                ),
            );
        }
    }
    let Some(owner_index) = owner_index else {
        return refuse::<K, A, _>(
            admission,
            format_args!(
                "procedural {noun} {id} references missing {noun} {owner}",
                noun = K::NOUN
            ),
        );
    };
    Ok(attach_at::<K, A>(model, admission, owner_index, owner, procedural)?.map(|_| ()))
}

/// Keyed views of one carrier family's arenas.
struct Indexes<'s> {
    /// Stored constructions by identity.
    constructions: IdentityPositions<'s>,
    /// Carriers by identity.
    carriers: IdentityPositions<'s>,
    /// Carriers by the construction their geometry names.
    named: IdentityPositions<'s>,
}

/// The first position among `candidates` whose identity is `identity`.
fn first_named<'c, A: ModelAdmission>(
    admission: &A,
    candidates: &[usize],
    identity: &str,
    of: impl Fn(usize) -> Option<&'c str>,
    operation: &'static str,
) -> Result<Option<usize>, A::Error> {
    for &position in candidates {
        admission.work(1, operation)?;
        if let Some(candidate) = of(position) {
            if admission.equal(candidate, identity, operation)? {
                return Ok(Some(position));
            }
        }
    }
    Ok(None)
}

/// Attach constructions in order, as one attachment each would, indexing the
/// arenas once. Returns each attachment's outcome in input order.
pub(super) fn attach_indexed<K: CarrierKind, A: ModelAdmission, O: Borrow<K::Owner>>(
    model: &mut Model,
    admission: &A,
    attachments: Vec<(O, K::Procedural)>,
) -> Result<Vec<Result<(), ProceduralCarrierError>>, A::Error> {
    let mut indexes = Indexes {
        constructions: admission.positions(K::INDEX)?,
        carriers: admission.positions(K::INDEX)?,
        named: admission.positions(K::INDEX)?,
    };
    for (position, existing) in K::procedurals(model).iter().enumerate() {
        admission.work(1, K::SCAN_CONSTRUCTIONS)?;
        let hash = admission.hash_identity(K::procedural_id(existing), K::INDEX)?;
        admission.push_position(&mut indexes.constructions, hash, position, K::INDEX)?;
    }
    for (position, carrier) in K::carriers(model).iter().enumerate() {
        admission.work(1, K::SCAN_CARRIERS)?;
        let hash = admission.hash_identity(K::carrier_id(carrier), K::INDEX)?;
        admission.push_position(&mut indexes.carriers, hash, position, K::INDEX)?;
        if let Some(construction) = K::carrier_construction(carrier) {
            let hash = admission.hash_identity(construction, K::INDEX)?;
            admission.push_position(&mut indexes.named, hash, position, K::INDEX)?;
        }
    }
    let mut outcomes = Vec::new();
    admission.reserve(&mut outcomes, attachments.len(), K::STORE)?;
    for (owner, procedural) in attachments {
        admission.work(1, K::SCAN_CONSTRUCTIONS)?;
        let outcome =
            attach_one_indexed::<K, A>(model, admission, &mut indexes, owner.borrow(), procedural)?;
        outcomes.push(outcome);
    }
    Ok(outcomes)
}

fn attach_one_indexed<K: CarrierKind, A: ModelAdmission>(
    model: &mut Model,
    admission: &A,
    indexes: &mut Indexes<'_>,
    owner: &K::Owner,
    procedural: K::Procedural,
) -> Result<Result<(), ProceduralCarrierError>, A::Error> {
    let id = K::procedural_id(&procedural);
    let hash = admission.hash_identity(id, K::INDEX)?;
    let stored = K::procedurals(model);
    let candidates = admission.positions_of(&indexes.constructions, hash, K::INDEX)?;
    if first_named(
        admission,
        candidates,
        id,
        |position| stored.get(position).map(K::procedural_id),
        K::COMPARE_CONSTRUCTIONS,
    )?
    .is_some()
    {
        return refuse::<K, A, _>(
            admission,
            format_args!("procedural {} construction {id} already exists", K::NOUN),
        );
    }
    let carriers = K::carriers(model);
    // Carriers naming this construction, in arena order: the first that is
    // not the owner refuses the attachment.
    for &position in admission.positions_of(&indexes.named, hash, K::INDEX)? {
        admission.work(1, K::SCAN_CARRIERS)?;
        let Some(carrier) = carriers.get(position) else {
            continue;
        };
        let names = match K::carrier_construction(carrier) {
            Some(construction) => admission.equal(construction, id, K::COMPARE_CONSTRUCTIONS)?,
            None => false,
        };
        if names
            && !admission.equal(
                K::carrier_id(carrier),
                K::owner_text(owner),
                K::COMPARE_OWNERS,
            )?
        {
            return refuse::<K, A, _>(
                admission,
                format_args!(
                    "procedural {noun} construction {id} already owns {noun} {}",
                    K::carrier_id(carrier),
                    noun = K::NOUN
                ),
            );
        }
    }
    let owner_hash = admission.hash_identity(K::owner_text(owner), K::INDEX)?;
    let Some(owner_index) = first_named(
        admission,
        admission.positions_of(&indexes.carriers, owner_hash, K::INDEX)?,
        K::owner_text(owner),
        |position| carriers.get(position).map(K::carrier_id),
        K::COMPARE_OWNERS,
    )?
    else {
        return refuse::<K, A, _>(
            admission,
            format_args!(
                "procedural {noun} {id} references missing {noun} {owner}",
                noun = K::NOUN
            ),
        );
    };
    let stored_position = K::procedurals(model).len();
    match attach_at::<K, A>(model, admission, owner_index, owner, procedural)? {
        Err(refused) => return Ok(Err(refused)),
        Ok(Attached::Owned) => {
            admission.push_position(&mut indexes.named, hash, owner_index, K::INDEX)?;
        }
        Ok(Attached::Direct) => {}
    }
    admission.push_position(&mut indexes.constructions, hash, stored_position, K::INDEX)?;
    Ok(Ok(()))
}

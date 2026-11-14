use crate::ids::native_stream;
use crate::loss::F3dLossCode;
use crate::records::feature::scope::{DesignParameterScope, DesignScopePayloadMut};
use crate::records::placement_construction::DesignPlacementConstruction;
use crate::records::sketch_placement::SketchPlacementMatrix;
use cadmpeg_ir::report::loss::LossNote;

pub(crate) fn bind(
    scopes: &mut [DesignParameterScope],
    placements: &[DesignPlacementConstruction],
) -> Vec<LossNote> {
    let mut losses = Vec::new();
    for scope in scopes {
        let stream = native_stream(&scope.id).map(str::to_owned);
        let id = scope.id.clone();
        let DesignScopePayloadMut::SpherePrimitive(Some(sphere)) = scope.payload_mut() else {
            continue;
        };
        let Some(reference) = sphere.placement_record_index else {
            continue;
        };
        let mut candidates = placements.iter().filter(|placement| {
            stream
                .as_deref()
                .is_some_and(|stream| native_stream(&placement.id) == Some(stream))
                && placement.record_index == u64::from(reference)
        });
        let placement = candidates.next().filter(|_| candidates.next().is_none());
        let frame = placement.and_then(|placement| {
            let strict = super::checked_solved_frame(placement);
            let assumed = strict.is_none()
                || placement.fields.inferred_x_axis.value != 0
                || placement.fields.inferred_z_axis.value != 0;
            let frame = strict.or_else(|| {
                placement.fields.matrix.or_else(|| {
                    placement
                        .fields
                        .identity
                        .filter(|identity| identity.value)
                        .map(|identity| crate::records::identity::Located {
                            value: SketchPlacementMatrix::IDENTITY,
                            offset: identity.offset,
                        })
                })
            })?;
            Some((frame, assumed))
        });
        match frame {
            Some((frame, assumed)) => {
                sphere.transform = frame.value;
                sphere.transform_offset = Some(frame.offset);
                if assumed {
                    losses.push(F3dLossCode::SpherePlacementFlagsAssumed.note(format!(
                        "Sphere {id} uses the stored solved placement despite unresolved placement flags (DR-550)."
                    )));
                }
            }
            None => {
                sphere.transform = SketchPlacementMatrix::IDENTITY;
                sphere.transform_offset = None;
                losses.push(F3dLossCode::SpherePlacementFrameAssumed.note(format!(
                    "Sphere {id} assumes an origin centre because placement {reference} has no unique usable same-stream solved frame (DR-550)."
                )));
            }
        }
        if sphere.diameter_owner_assumed {
            losses.push(F3dLossCode::SphereDiameterOwnerAssumed.note(format!(
                "Sphere {id} reads its referenced positive scalar as the diameter despite unresolved owner binding (DR-551)."
            )));
        }
        if sphere.diameter_source_assumed {
            losses.push(F3dLossCode::SphereDiameterSourceAssumed.note(format!(
                "Sphere {id} uses its evaluated diameter because its parameter source has no unique same-stream definition (DR-551)."
            )));
        }
    }
    losses
}

#[cfg(test)]
mod tests;

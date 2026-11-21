// SPDX-License-Identifier: Apache-2.0
//! Rewrite the identities owned by these fields.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use super::{BodyTopology, HistoryBody, HistoryStateRef};
use crate::schema::rewrite::typed::{IdentityMap, RewriteIdentities};

rewrite_scalar!(HistoryStateRef);
rewrite_record!(BodyTopology, []; {body, regions, shells, faces, loops, coedges, edges, vertices, surfaces, curves, pcurves, points});

/// The references of a history body that resolve against the model are its
/// producer and consumer. The records of the nest resolve inside the nest and
/// are rewritten with the body but never visited as references.
impl RewriteIdentities for HistoryBody {
    fn visit_identity_references(
        &self,
        ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        let _depth = ctx.enter_nested("walk typed reference fields")?;
        self.id.visit_identity_references(ctx, visitor)?;
        self.produced_by.visit_identity_references(ctx, visitor)?;
        self.consumed_by.visit_identity_references(ctx, visitor)
    }

    fn rewrite_identities<F>(
        self,
        ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<Self, CodecError>
    where
        F: FnMut(&str) -> Result<String, CodecError>,
    {
        let _depth = ctx.enter_nested("typed rewrite record")?;
        let Self {
            id,
            produced_by,
            consumed_by,
            state,
            topology,
        } = self;
        Ok(Self {
            id: id.rewrite_identities(ctx, map)?,
            produced_by: produced_by.rewrite_identities(ctx, map)?,
            consumed_by: consumed_by.rewrite_identities(ctx, map)?,
            state: state.rewrite_identities(ctx, map)?,
            topology: topology.rewrite_identities(ctx, map)?,
        })
    }
}

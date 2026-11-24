// SPDX-License-Identifier: Apache-2.0
//! Solid Edge document decode.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{DecodeBody, Decoded};
use cadmpeg_ir::document::SourceMeta;
use cadmpeg_ir::hash::digest::Sha256Digest;
use cadmpeg_ir::ids::ProductDefinitionId;
use cadmpeg_ir::products::{ExternalDocument, ProductDefinition};
use cadmpeg_ir::report::decode::DecodeTransfer;
use cadmpeg_ir::{CadIr, SourceFidelity};

use crate::{container, dialect, features, loss::SolidEdgeLossCode, settings};

pub(crate) fn decode(ctx: &DecodeContext<'_>, root: View<'_>) -> Result<Decoded, CodecError> {
    let snapshot = container::open(ctx, root)?;
    let mut ir = CadIr::empty();
    ir.source = Some(SourceMeta::classified(
        dialect::layers(),
        std::collections::BTreeMap::default(),
    ));
    let mut body = DecodeBody::new(DecodeTransfer::at(ctx.depth(), false));
    body.losses.extend(
        snapshot.deviations().map(|deviation| {
            SolidEdgeLossCode::ContainerNoncanonical.note(deviation.description())
        }),
    );
    if !ctx.container_only() {
        let definition = root_definition();
        let mut product = ProductDefinition::root(definition.clone(), ExternalDocument::default());
        let cluster = match snapshot.stream(settings::CLUSTER_STREAM) {
            Some(entry) => match snapshot.open(ctx, entry) {
                Ok(cluster) => Ok(cluster),
                Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
                Err(error) => Err(format!(
                    "the {} stream could not be read: {error}",
                    settings::CLUSTER_STREAM
                )),
            },
            None => Err(format!(
                "the document has no {} stream",
                settings::CLUSTER_STREAM
            )),
        };
        product.units = match &cluster {
            Ok(cluster) => {
                let sets = settings::property_sets(ctx, cluster.window());
                settings::unit_system(&sets, &mut body.losses)
            }
            Err(reason) => {
                body.losses
                    .push(SolidEdgeLossCode::UnitsUnread.note(format!(
                        "{reason}; the document unit system is not transferred"
                    )));
                None
            }
        };
        ir.model.product_definitions.push(product);
        if let Ok(cluster) = cluster {
            features::project(
                ctx,
                &snapshot,
                cluster,
                &definition,
                &mut ir,
                &mut body.losses,
            )?;
        }
        body.losses
            .push(SolidEdgeLossCode::BodiesPending.note("Solid Edge body decoding is pending"));
    }
    Ok(Decoded {
        ir,
        body,
        source_fidelity: SourceFidelity::new(Sha256Digest::digest(root.window())),
    })
}

fn root_definition() -> ProductDefinitionId {
    ProductDefinitionId::compose(
        &cadmpeg_ir::identity_namespace!("solidedge", "model", "product-definition"),
        cadmpeg_ir::identity_key!("root"),
    )
}

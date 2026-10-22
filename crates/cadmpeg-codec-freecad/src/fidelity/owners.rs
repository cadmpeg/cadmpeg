// SPDX-License-Identifier: Apache-2.0
//! Source ownership from the closed FCStd native arena schemas.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::Identity;
use cadmpeg_ir::source_fidelity::b1::provenance::{NativeOwnerResolver, NativeOwnership};
use cadmpeg_ir::CadIr;

pub(crate) struct Owners;

impl NativeOwnerResolver for Owners {
    fn visit_owners(
        &self,
        _ctx: &DecodeContext<'_>,
        ir: &CadIr,
        visitor: &mut dyn FnMut(NativeOwnership) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        if let Some(native) = ir.native.namespace("fcstd") {
            for (arena, records) in native.arenas() {
                if arena == "unknowns" {
                    continue;
                }
                let field = match arena.as_str() {
                    "document" | "objects" | "entries" | "gui_documents" => None,
                    "extensions" | "properties" | "gui_properties" => Some(("owner", false)),
                    "shape_payloads" | "element_maps" => Some(("property", false)),
                    "carrier_census" => Some(("payload", false)),
                    "product_nodes" | "joints" | "drawings" | "annotations" | "applications"
                    | "attachments" | "design_census" => Some(("object", false)),
                    "gui_view_providers" => Some(("object", true)),
                    "string_tables" => Some(("owner_property", true)),
                    _ => {
                        return Err(CodecError::malformed(format_args!(
                            "FCStd arena {arena} has no typed source-owner rule"
                        )))
                    }
                };
                for record in records {
                    let owner = match field {
                        None => None,
                        Some((name, optional)) => match record.field(name) {
                            Some(serde_json::Value::String(value)) => {
                                Some(Identity::new(value).map_err(CodecError::malformed)?)
                            }
                            None | Some(serde_json::Value::Null) if optional => None,
                            _ => {
                                return Err(CodecError::malformed(format_args!(
                                    "FCStd {arena} record {} has an invalid typed {name} reference",
                                    record.id()
                                )))
                            }
                        },
                    };
                    visitor(NativeOwnership::new(
                        Identity::new(record.id()).map_err(CodecError::malformed)?,
                        owner,
                    ))?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;

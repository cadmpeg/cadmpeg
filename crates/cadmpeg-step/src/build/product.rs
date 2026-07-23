// SPDX-License-Identifier: Apache-2.0
//! STEP product structure and occurrence graph emission.

use std::collections::HashMap;

use cadmpeg_ir::ids::{OccurrenceId, ProductId};
use cadmpeg_ir::product::OccurrenceParent;
use cadmpeg_ir::report::{LossCategory, LossCode, Severity};

use crate::geometry;
use crate::writer::{refs, string, Ref};

use super::{is_identity, is_rigid_transform, Builder};

impl Builder<'_> {
    pub(super) fn emit_product_structure(&mut self) -> Ref {
        let name = self
            .ir
            .model
            .bodies
            .first()
            .and_then(|b| b.name.clone())
            .unwrap_or_else(|| "cadmpeg_model".to_string());

        let (application, protocol, year) = self.schema.application_protocol();
        let app_ctx = self
            .emitter
            .emit("APPLICATION_CONTEXT", &string(application));
        self.emitter.emit(
            "APPLICATION_PROTOCOL_DEFINITION",
            &format!(
                "{},{},{year},{app_ctx}",
                string("international standard"),
                string(protocol)
            ),
        );
        let prod_ctx = self.emitter.emit(
            "PRODUCT_CONTEXT",
            &format!("'',{app_ctx},{}", string("mechanical")),
        );
        let product = self.emitter.emit(
            "PRODUCT",
            &format!("{},{},'',({prod_ctx})", string(&name), string(&name)),
        );
        let formation = self
            .emitter
            .emit("PRODUCT_DEFINITION_FORMATION", &format!("'','',{product}"));
        let pd_ctx = self.emitter.emit(
            "PRODUCT_DEFINITION_CONTEXT",
            &format!(
                "{},{app_ctx},{}",
                string("part definition"),
                string("design")
            ),
        );
        let product_def = self.emitter.emit(
            "PRODUCT_DEFINITION",
            &format!("{},'',{formation},{pd_ctx}", string("design")),
        );
        self.emitter
            .emit("PRODUCT_DEFINITION_SHAPE", &format!("'','',{product_def}"))
    }

    pub(super) fn emit_product_graph(&mut self, context: Ref) {
        let (application, protocol, year) = self.schema.application_protocol();
        let app_context = self
            .emitter
            .emit("APPLICATION_CONTEXT", &string(application));
        self.emitter.emit(
            "APPLICATION_PROTOCOL_DEFINITION",
            &format!(
                "{},{},{year},{app_context}",
                string("international standard"),
                string(protocol)
            ),
        );
        let product_context = self.emitter.emit(
            "PRODUCT_CONTEXT",
            &format!("'',{app_context},{}", string("mechanical")),
        );
        let definition_context = self.emitter.emit(
            "PRODUCT_DEFINITION_CONTEXT",
            &format!(
                "{},{app_context},{}",
                string("part definition"),
                string("design")
            ),
        );

        let ir = self.ir;
        let products = &ir.model.products;
        let occurrences = &ir.model.product_occurrences;
        let occurrence_products = occurrences
            .iter()
            .map(|occurrence| (occurrence.id.clone(), occurrence.product.clone()))
            .collect::<HashMap<OccurrenceId, ProductId>>();
        let mut product_origins = HashMap::<ProductId, Ref>::new();
        for product in products {
            product_origins.insert(
                product.id.clone(),
                geometry::placement(
                    &mut self.emitter,
                    cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                    cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
                    cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
                ),
            );
        }
        let mut representation_placements = HashMap::<ProductId, Vec<Ref>>::new();
        let mut occurrence_placements = HashMap::<OccurrenceId, (Ref, Ref)>::new();
        for occurrence in occurrences {
            let OccurrenceParent::Occurrence { occurrence: parent } = &occurrence.parent else {
                continue;
            };
            let Some(parent_product) = occurrence_products.get(parent) else {
                continue;
            };
            let Some(&from) = product_origins.get(&occurrence.product) else {
                continue;
            };
            if !is_rigid_transform(&occurrence.transform.rows) {
                continue;
            }
            let rows = occurrence.transform.rows;
            let to = geometry::placement(
                &mut self.emitter,
                cadmpeg_ir::math::Point3::new(rows[0][3], rows[1][3], rows[2][3]),
                cadmpeg_ir::math::Vector3::new(rows[0][2], rows[1][2], rows[2][2]),
                cadmpeg_ir::math::Vector3::new(rows[0][0], rows[1][0], rows[2][0]),
            );
            representation_placements
                .entry(parent_product.clone())
                .or_default()
                .push(to);
            occurrence_placements.insert(occurrence.id.clone(), (from, to));
        }
        let mut definitions = HashMap::<ProductId, Ref>::new();
        let mut representations = HashMap::<ProductId, Ref>::new();
        for product in products {
            let name = product.name.as_deref().unwrap_or(&product.product_id);
            let product_ref = self.emitter.emit(
                "PRODUCT",
                &format!(
                    "{},{},'',({product_context})",
                    string(&product.product_id),
                    string(name)
                ),
            );
            let formation = self.emitter.emit(
                "PRODUCT_DEFINITION_FORMATION",
                &format!("'','',{product_ref}"),
            );
            let definition = self.emitter.emit(
                "PRODUCT_DEFINITION",
                &format!(
                    "{},'',{formation},{definition_context}",
                    string(&product.product_id)
                ),
            );
            let shape = self
                .emitter
                .emit("PRODUCT_DEFINITION_SHAPE", &format!("'','',{definition}"));
            self.default_product_definition_shape.get_or_insert(shape);
            let mut body_items = product
                .bodies
                .iter()
                .flat_map(|body| {
                    self.body_item_refs
                        .get(body.as_str())
                        .into_iter()
                        .flatten()
                        .copied()
                })
                .collect::<Vec<_>>();
            if let Some(origin) = product_origins.get(&product.id) {
                body_items.push(*origin);
            }
            if let Some(placements) = representation_placements.get(&product.id) {
                body_items.extend(placements);
            }
            let representation = self.emitter.emit(
                "SHAPE_REPRESENTATION",
                &format!("{},{},{context}", string(name), refs(&body_items)),
            );
            self.emitter.emit(
                "SHAPE_DEFINITION_REPRESENTATION",
                &format!("{shape},{representation}"),
            );
            definitions.insert(product.id.clone(), definition);
            representations.insert(product.id.clone(), representation);
        }

        for occurrence in occurrences {
            let OccurrenceParent::Occurrence { occurrence: parent } = &occurrence.parent else {
                if !is_identity(&occurrence.transform.rows) {
                    self.loss(
                        LossCode::BodyTransformNotApplied,
                        LossCategory::Topology,
                        Severity::Warning,
                        format!(
                            "root occurrence '{}' has a non-identity placement",
                            occurrence.id
                        ),
                    );
                }
                continue;
            };
            let Some(parent_product) = occurrence_products.get(parent) else {
                self.loss(
                    LossCode::TopologyNotTransferred,
                    LossCategory::Topology,
                    Severity::Warning,
                    format!("occurrence '{}' has an unresolved parent", occurrence.id),
                );
                continue;
            };
            let Some((
                &parent_definition,
                &child_definition,
                &parent_representation,
                &child_representation,
            )) = definitions
                .get(parent_product)
                .zip(definitions.get(&occurrence.product))
                .zip(representations.get(parent_product))
                .zip(representations.get(&occurrence.product))
                .map(|(((a, b), c), d)| (a, b, c, d))
            else {
                continue;
            };
            if !is_rigid_transform(&occurrence.transform.rows) {
                self.loss(
                    LossCode::BodyTransformNotApplied,
                    LossCategory::Topology,
                    Severity::Warning,
                    format!("occurrence '{}' placement is not rigid", occurrence.id),
                );
                continue;
            }
            let occurrence_name = occurrence.name.as_deref().unwrap_or(occurrence.id.as_str());
            let usage = self.emitter.emit(
                "NEXT_ASSEMBLY_USAGE_OCCURRENCE",
                &format!(
                    "{},{},'',{parent_definition},{child_definition},$",
                    string(occurrence.id.as_str()),
                    string(occurrence_name)
                ),
            );
            let usage_shape = self
                .emitter
                .emit("PRODUCT_DEFINITION_SHAPE", &format!("'','',{usage}"));
            let Some(&(from, to)) = occurrence_placements.get(&occurrence.id) else {
                continue;
            };
            let transform = self
                .emitter
                .emit("ITEM_DEFINED_TRANSFORMATION", &format!("'','',{from},{to}"));
            let relationship = self.emitter.emit_raw(
                "REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION",
                &format!(
                    "( REPRESENTATION_RELATIONSHIP('','',{child_representation},{parent_representation}) REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION({transform}) SHAPE_REPRESENTATION_RELATIONSHIP() )"
                ),
            );
            self.emitter.emit(
                "CONTEXT_DEPENDENT_SHAPE_REPRESENTATION",
                &format!("{relationship},{usage_shape}"),
            );
        }
    }
}

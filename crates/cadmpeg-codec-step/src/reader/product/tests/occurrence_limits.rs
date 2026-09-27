// SPDX-License-Identifier: Apache-2.0
//! Collection limits for product occurrence expansion.

#[test]
fn root_occurrence_items_refuse_collection_limit() {
    super::product_collection_refuses("step_root_occurrence_items");
}

#[test]
fn root_occurrence_path_map_refuses_collection_limit() {
    super::product_collection_refuses("step_root_occurrence_path_map");
}

#[test]
fn root_occurrence_path_members_refuse_collection_limit() {
    super::product_collection_refuses("step_root_occurrence_path_members");
}

#[test]
fn usage_instance_counts_refuse_collection_limit() {
    super::product_collection_refuses("step_usage_instance_counts");
}

#[test]
fn child_occurrence_ordinals_refuse_collection_limit() {
    super::product_collection_refuses("step_child_occurrence_ordinals");
}

#[test]
fn missing_placement_reports_refuse_collection_limit() {
    super::product_collection_refuses("step_missing_placement_reports");
}

#[test]
fn child_occurrence_items_refuse_collection_limit() {
    super::product_collection_refuses("step_child_occurrence_items");
}

#[test]
fn child_occurrence_path_members_refuse_collection_limit() {
    super::product_collection_refuses("step_child_occurrence_path_members");
}

#[test]
fn child_occurrence_path_map_refuses_collection_limit() {
    super::product_collection_refuses("step_child_occurrence_path_map");
}

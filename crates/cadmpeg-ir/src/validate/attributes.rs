// SPDX-License-Identifier: Apache-2.0
//! Domains and typed references of source attributes.

use crate::attributes::{AttributeJson, AttributeValue};
use crate::index::{ModelIndex, StandardIndex};
use crate::report::{
    check::{Check, Finding},
    Severity,
};
use crate::CadIr;
use cadmpeg_core::{decode::DecodeContext, CodecError};

pub(super) fn check_values(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    ids: &ModelIndex<'_>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    for attribute in &ir.model.attributes {
        for value in attribute.values.iter().flatten() {
            match value {
                AttributeValue::Reference(object) if object.is_empty() => {
                    invalid(
                        findings,
                        attribute.id.as_str(),
                        "an absent local object uses empty_reference",
                    );
                }
                AttributeValue::SubReference {
                    object,
                    subelements,
                } => {
                    if subelements.is_empty() {
                        invalid(
                            findings,
                            attribute.id.as_str(),
                            "a subreference requires a subelement",
                        );
                    }
                    if let Some(object) = object {
                        if object.is_empty() {
                            invalid(
                                findings,
                                attribute.id.as_str(),
                                "an absent linked object uses null",
                            );
                        } else if !ids.contains(object, StandardIndex) {
                            super::record_finding(
                                findings,
                                Check::ReferentialIntegrity,
                                Severity::Error,
                                Some(attribute.id.as_str()),
                                format_args!(
                                    "references missing local attribute object `{object}`"
                                ),
                            );
                        }
                    }
                }
                AttributeValue::ExternalReference {
                    object: Some(object),
                    ..
                } if object.is_empty() => {
                    invalid(
                        findings,
                        attribute.id.as_str(),
                        "an absent external object uses null",
                    );
                }
                AttributeValue::Expression(value) => {
                    for parameter in value.expression.dependencies() {
                        if !ir.model.parameters.iter().any(|held| &held.id == parameter) {
                            super::record_finding(findings, Check::ReferentialIntegrity, Severity::Error,
                                Some(attribute.id.as_str()), format_args!("references missing attribute expression parameter `{parameter}`"));
                        }
                    }
                }
                AttributeValue::Json(value) => {
                    check_json(ctx, attribute.id.as_str(), value, findings)?;
                }
                _ => {}
            }
        }
    }
    Ok(())
}

fn invalid(findings: &mut Vec<Finding>, owner: &str, message: &'static str) {
    super::record_finding(
        findings,
        Check::PayloadIntegrity,
        Severity::Error,
        Some(owner),
        format_args!("{message}"),
    );
}

fn check_json(
    ctx: &DecodeContext<'_>,
    owner: &str,
    value: &AttributeJson,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let _depth = ctx.enter_nested("validate attribute JSON")?;
    match value {
        AttributeJson::Unsigned(value) if i64::try_from(*value).is_ok() => {
            invalid(
                findings,
                owner,
                "attribute unsigned integer belongs to the signed domain",
            );
        }
        AttributeJson::Array(values) => {
            for value in values {
                check_json(ctx, owner, value, findings)?;
            }
        }
        AttributeJson::Object(values) => {
            for value in values.values() {
                check_json(ctx, owner, value, findings)?;
            }
        }
        _ => {}
    }
    Ok(())
}

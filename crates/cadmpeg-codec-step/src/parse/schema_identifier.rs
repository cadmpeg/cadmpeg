// SPDX-License-Identifier: Apache-2.0
//! Part 21 schema identifier classification.
//!
//! A schema identifier is a schema name and an optional ISO/IEC 8824-1 object
//! identifier. The classifier is total: an identifier is valid, recoverable
//! under its schema name alone, or invalid. `valid_schema_identifier` is the
//! strict predicate; only the `FILE_SCHEMA` admission point reads the
//! recoverable form.

/// The admission form of one schema identifier.
pub(super) enum SchemaIdentifierForm<'a> {
    /// The schema name and the optional object identifier are both valid.
    Valid,
    /// The object identifier parses, and one component number is negative.
    ObjectIdentifierOutOfRange {
        /// The schema name, without the object identifier.
        name: &'a str,
        /// The first component in source order whose number is negative.
        component: &'a str,
    },
    /// The identifier does not parse.
    Invalid,
}

pub(super) fn schema_identifier_form(identifier: &str) -> SchemaIdentifierForm<'_> {
    let identifier = identifier.trim();
    if identifier.is_empty() || identifier.chars().count() > 1024 {
        return SchemaIdentifierForm::Invalid;
    }
    let (name, object_identifier) = match identifier.split_once('{') {
        Some((name, object_identifier)) => {
            let Some(object_identifier) = object_identifier.strip_suffix('}') else {
                return SchemaIdentifierForm::Invalid;
            };
            (name.trim_end(), Some(object_identifier))
        }
        None => (identifier, None),
    };
    if !valid_schema_name(name) {
        return SchemaIdentifierForm::Invalid;
    }
    let Some(object_identifier) = object_identifier else {
        return SchemaIdentifierForm::Valid;
    };
    match schema_object_identifier_form(object_identifier) {
        ObjectIdentifierForm::Valid => SchemaIdentifierForm::Valid,
        ObjectIdentifierForm::ComponentOutOfRange(component) => {
            SchemaIdentifierForm::ObjectIdentifierOutOfRange { name, component }
        }
        ObjectIdentifierForm::Invalid => SchemaIdentifierForm::Invalid,
    }
}

pub(super) fn valid_schema_identifier(identifier: &str) -> bool {
    matches!(
        schema_identifier_form(identifier),
        SchemaIdentifierForm::Valid
    )
}

fn valid_schema_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes.next().is_some_and(|byte| byte.is_ascii_alphabetic())
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

/// The admission form of one object identifier.
enum ObjectIdentifierForm<'a> {
    /// Every component is valid, and the root components hold their rule.
    Valid,
    /// Every component parses, and this component's number is negative.
    ComponentOutOfRange(&'a str),
    /// The object identifier does not parse.
    Invalid,
}

fn schema_object_identifier_form(value: &str) -> ObjectIdentifierForm<'_> {
    let mut components = value.split_whitespace();
    let Some(first) = components.next() else {
        return ObjectIdentifierForm::Invalid;
    };
    let Some(second) = components.next() else {
        return ObjectIdentifierForm::Invalid;
    };
    let mut out_of_range = None;
    for component in [first, second].into_iter().chain(components) {
        match schema_oid_component_form(component) {
            ComponentForm::Valid => {}
            ComponentForm::Negative => out_of_range = out_of_range.or(Some(component)),
            ComponentForm::Invalid => return ObjectIdentifierForm::Invalid,
        }
    }
    if !valid_schema_oid_root(first, second) {
        return ObjectIdentifierForm::Invalid;
    }
    out_of_range.map_or(ObjectIdentifierForm::Valid, |component| {
        ObjectIdentifierForm::ComponentOutOfRange(component)
    })
}

/// The admission form of one object identifier component.
enum ComponentForm {
    /// The component is a name, a number, or a named number.
    Valid,
    /// The component parses, and its number carries a minus sign. An object
    /// identifier component number is non-negative, so the number is out of
    /// range.
    Negative,
    /// The component does not parse.
    Invalid,
}

fn schema_oid_component_form(component: &str) -> ComponentForm {
    if valid_schema_oid_name(component) {
        return ComponentForm::Valid;
    }
    let Some((name, number)) = component.split_once('(') else {
        return schema_oid_number_form(component);
    };
    let Some(number) = number.strip_suffix(')') else {
        return ComponentForm::Invalid;
    };
    if !valid_schema_oid_name(name) {
        return ComponentForm::Invalid;
    }
    if valid_schema_oid_name(number) {
        return ComponentForm::Valid;
    }
    schema_oid_number_form(number)
}

fn schema_oid_number_form(value: &str) -> ComponentForm {
    if valid_schema_oid_number(value) {
        return ComponentForm::Valid;
    }
    if value.strip_prefix('-').is_some_and(valid_schema_oid_number) {
        return ComponentForm::Negative;
    }
    ComponentForm::Invalid
}

fn valid_schema_oid_number(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && (value == "0" || !value.starts_with('0'))
}

fn valid_schema_oid_name(value: &str) -> bool {
    let mut bytes = value.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    if !first.is_ascii_lowercase() {
        return false;
    }
    let mut previous_hyphen = false;
    for byte in bytes {
        if byte.is_ascii_alphabetic() || byte.is_ascii_digit() {
            previous_hyphen = false;
        } else if byte == b'-' && !previous_hyphen {
            previous_hyphen = true;
        } else {
            return false;
        }
    }
    !value.ends_with('-')
}

fn valid_schema_oid_root(first: &str, second: &str) -> bool {
    let first = match schema_oid_explicit_root(first) {
        Some(Ok(value)) => value,
        Some(Err(())) => return false,
        None => {
            let Some(value) = schema_oid_named_root_value(first) else {
                return true;
            };
            value
        }
    };
    if first > 2 {
        return false;
    }
    if first < 2 {
        if let Some(second) = schema_oid_component_number_text(second) {
            return second.len() < 2 || (second.len() == 2 && second.as_bytes()[0] <= b'3');
        }
    }
    true
}

fn schema_oid_named_root_value(component: &str) -> Option<u8> {
    match component {
        "itu-t" | "ccitt" => Some(0),
        "iso" => Some(1),
        "joint-iso-itu-t" | "joint-iso-ccitt" => Some(2),
        _ => None,
    }
}

fn schema_oid_explicit_root(component: &str) -> Option<Result<u8, ()>> {
    let value = schema_oid_component_number_text(component)?;
    Some(match value {
        "0" => Ok(0),
        "1" => Ok(1),
        "2" => Ok(2),
        _ => Err(()),
    })
}

fn schema_oid_component_number_text(component: &str) -> Option<&str> {
    if valid_schema_oid_number(component) {
        return Some(component);
    }
    let (name, number) = component.split_once('(')?;
    let number = number.strip_suffix(')')?;
    (valid_schema_oid_name(name) && valid_schema_oid_number(number)).then_some(number)
}

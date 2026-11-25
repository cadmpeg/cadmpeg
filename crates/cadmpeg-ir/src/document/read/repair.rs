// SPDX-License-Identifier: Apache-2.0
//! The repairs a record's value takes from the refusal its type states: a
//! member or variant name that differs only in letter case from exactly one
//! name of its type, a member the type does not know, and an omitted
//! collection (§3.3, Reading CADIR).

use serde_json::Value;

/// A refusal the reader repairs, as `serde` words it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Refusal {
    /// An unknown field, with the names its type expects.
    UnknownField { name: String, expected: Vec<String> },
    /// An unknown variant, with the names its type expects.
    UnknownVariant { name: String, expected: Vec<String> },
    /// A missing field.
    MissingField(String),
}

/// The refusal `message` states, where the reader repairs it.
pub(super) fn refusal(message: &str) -> Option<Refusal> {
    let named = |marker: &str| {
        let at = message.find(marker)?;
        let mut names = quoted(&message[at + marker.len() - 1..]);
        (!names.is_empty()).then(|| {
            let name = names.remove(0);
            (name, names)
        })
    };
    if let Some((name, expected)) = named("unknown field `") {
        return Some(Refusal::UnknownField { name, expected });
    }
    if let Some((name, expected)) = named("unknown variant `") {
        return Some(Refusal::UnknownVariant { name, expected });
    }
    named("missing field `").map(|(name, _)| Refusal::MissingField(name))
}

/// Every name between backticks in `text`, in order.
fn quoted(text: &str) -> Vec<String> {
    text.split('`')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect()
}

/// The one name of `expected` that equals `name` but for letter case.
pub(super) fn case_variant<'a>(name: &str, expected: &'a [String]) -> Option<&'a str> {
    let mut matches = expected
        .iter()
        .filter(|candidate| *candidate != name && candidate.eq_ignore_ascii_case(name));
    let first = matches.next()?;
    matches.next().is_none().then_some(first.as_str())
}

/// The path, as object keys and array indices, of each object in `value`,
/// in depth-first order.
fn objects(value: &Value) -> Vec<Vec<Step>> {
    let mut found = Vec::new();
    let mut stack = vec![(value, Vec::new())];
    while let Some((value, path)) = stack.pop() {
        match value {
            Value::Object(object) => {
                for (key, child) in object.iter().rev() {
                    let mut child_path = path.clone();
                    child_path.push(Step::Key(key.clone()));
                    stack.push((child, child_path));
                }
                found.push(path);
            }
            Value::Array(items) => {
                for (index, child) in items.iter().enumerate().rev() {
                    let mut child_path = path.clone();
                    child_path.push(Step::Index(index));
                    stack.push((child, child_path));
                }
            }
            _ => {}
        }
    }
    found
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Step {
    Key(String),
    Index(usize),
}

fn at_mut<'a>(value: &'a mut Value, path: &[Step]) -> Option<&'a mut Value> {
    path.iter().try_fold(value, |value, step| match step {
        Step::Key(key) => value.get_mut(key.as_str()),
        Step::Index(index) => value.get_mut(*index),
    })
}

fn at<'a>(value: &'a Value, path: &[Step]) -> Option<&'a Value> {
    path.iter().try_fold(value, |value, step| match step {
        Step::Key(key) => value.get(key.as_str()),
        Step::Index(index) => value.get(*index),
    })
}

/// What a member repair did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum MemberRepair {
    /// The member is read under the one name its letter case varies.
    Renamed { from: String, to: String },
    /// The member is not stated.
    Ignored(String),
    /// The member's letter case varies the name `to`, which the object also
    /// states. Where both hold one value, `to` is kept; otherwise neither is.
    Restated {
        from: String,
        to: String,
        one_value: bool,
    },
}

/// Repairs the member `name` that a type refused, in the object of `value`
/// that holds it and the most of the `expected` names: renames it to the one
/// expected name its letter case varies, or removes it. Where the object also
/// states that name, the two are one member stated twice.
pub(super) fn member(value: &mut Value, name: &str, expected: &[String]) -> Option<MemberRepair> {
    let holder = objects(value)
        .into_iter()
        .filter_map(|path| {
            let object = at(value, &path)?.as_object()?;
            object.contains_key(name).then(|| {
                let known = object
                    .keys()
                    .filter(|key| expected.iter().any(|name| name == *key))
                    .count();
                (known, path)
            })
        })
        .fold(None::<(usize, Vec<Step>)>, |best, candidate| match best {
            Some(best) if best.0 >= candidate.0 => Some(best),
            _ => Some(candidate),
        })?
        .1;
    let object = at_mut(value, &holder)?.as_object_mut()?;
    let member = object.remove(name)?;
    match case_variant(name, expected) {
        Some(to) if object.contains_key(to) => {
            let one_value = object.get(to) == Some(&member);
            if !one_value {
                object.remove(to);
            }
            Some(MemberRepair::Restated {
                from: name.to_owned(),
                to: to.to_owned(),
                one_value,
            })
        }
        Some(to) => {
            object.insert(to.to_owned(), member);
            Some(MemberRepair::Renamed {
                from: name.to_owned(),
                to: to.to_owned(),
            })
        }
        None => Some(MemberRepair::Ignored(name.to_owned())),
    }
}

/// Restates the first string or object key of `value` that is `name` as
/// `to`. Returns whether it found one.
pub(super) fn variant(value: &mut Value, name: &str, to: &str) -> bool {
    match value {
        Value::String(text) if text == name => {
            to.clone_into(text);
            true
        }
        Value::Object(object) => {
            if let Some(Value::String(tag)) = object.get_mut("kind") {
                if tag == name {
                    to.clone_into(tag);
                    return true;
                }
            }
            if let Some(member) = object.remove(name) {
                object.insert(to.to_owned(), member);
                return true;
            }
            object.values_mut().any(|child| variant(child, name, to))
        }
        Value::Array(items) => items.iter_mut().any(|child| variant(child, name, to)),
        _ => false,
    }
}

/// The places an omitted collection `name` can be stated: each object of
/// `value` that lacks it, in depth-first order, at most `limit`.
pub(super) fn collection_sites(value: &Value, name: &str, limit: usize) -> Vec<Site> {
    objects(value)
        .into_iter()
        .filter(|path| {
            at(value, path)
                .and_then(Value::as_object)
                .is_some_and(|object| !object.contains_key(name))
        })
        .take(limit)
        .map(Site)
        .collect()
}

/// An object of a record's value.
#[derive(Debug, Clone)]
pub(super) struct Site(Vec<Step>);

/// States `filler` as the member `name` of the object at `site`.
pub(super) fn insert(value: &mut Value, site: &Site, name: &str, filler: Value) {
    if let Some(object) = at_mut(value, &site.0).and_then(Value::as_object_mut) {
        object.insert(name.to_owned(), filler);
    }
}

/// Removes the member `name` of the object at `site`.
pub(super) fn remove(value: &mut Value, site: &Site, name: &str) {
    if let Some(object) = at_mut(value, &site.0).and_then(Value::as_object_mut) {
        object.remove(name);
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{member, refusal, variant, MemberRepair, Refusal};

    #[test]
    fn serde_refusals_name_the_member_or_variant_and_the_expected_names() {
        assert_eq!(
            refusal("key a: unknown field `Radius`, expected `radius` or `center`"),
            Some(Refusal::UnknownField {
                name: "Radius".into(),
                expected: vec!["radius".into(), "center".into()],
            })
        );
        assert_eq!(
            refusal("unknown variant `Plane`, expected one of `plane`, `cone`"),
            Some(Refusal::UnknownVariant {
                name: "Plane".into(),
                expected: vec!["plane".into(), "cone".into()],
            })
        );
        assert_eq!(
            refusal("missing field `loops`"),
            Some(Refusal::MissingField("loops".into()))
        );
        assert_eq!(refusal("invalid type: string, expected f64"), None);
    }

    #[test]
    fn a_member_is_renamed_where_its_case_varies_one_name_and_removed_otherwise() {
        let mut value = json!({"id": "a", "inner": {"Radius": 1.0, "center": [0.0]}});
        let expected = vec!["radius".to_owned(), "center".to_owned()];
        assert_eq!(
            member(&mut value, "Radius", &expected),
            Some(MemberRepair::Renamed {
                from: "Radius".into(),
                to: "radius".into()
            })
        );
        assert_eq!(
            value,
            json!({"id": "a", "inner": {"radius": 1.0, "center": [0.0]}})
        );
        let mut value = json!({"id": "a", "extra": 1});
        assert_eq!(
            member(&mut value, "extra", &["id".to_owned()]),
            Some(MemberRepair::Ignored("extra".into()))
        );
        assert_eq!(value, json!({"id": "a"}));
    }

    #[test]
    fn a_variant_name_is_restated_where_it_is_first_stated() {
        let mut value = json!({"kind": "Plane", "items": ["Plane"]});
        assert!(variant(&mut value, "Plane", "plane"));
        assert_eq!(value, json!({"kind": "plane", "items": ["Plane"]}));
    }
}

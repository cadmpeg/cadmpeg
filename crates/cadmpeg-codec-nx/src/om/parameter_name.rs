// SPDX-License-Identifier: Apache-2.0
//! Exact parameter spelling with its parsed index and qualifier boundary.

/// A name parsed once at construction. `u32` requires canonical parameter syntax;
/// `Option<u32>` also admits ordinary expression names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ParameterName<S, I = Option<u32>> {
    spelling: S,
    index: I,
    qualifier_start: Option<usize>,
}

impl<S: crate::immutable_text::ImmutableText> ParameterName<S> {
    pub(crate) fn new(spelling: S) -> Self {
        let (index, qualifier_start) = canonical_parts(spelling.as_ref())
            .map_or((None, None), |(index, qualifier)| (Some(index), qualifier));
        Self {
            spelling,
            index,
            qualifier_start,
        }
    }
}

impl<S: crate::immutable_text::ImmutableText> ParameterName<S, u32> {
    pub(crate) fn parse(spelling: S) -> Option<Self> {
        let (index, qualifier_start) = canonical_parts(spelling.as_ref())?;
        Some(Self {
            spelling,
            index,
            qualifier_start,
        })
    }
}

impl<S: crate::immutable_text::ImmutableText, I: Copy> ParameterName<S, I> {
    pub(crate) fn as_str(&self) -> &str {
        self.spelling.as_ref()
    }

    pub(crate) fn index(&self) -> I {
        self.index
    }

    pub(crate) fn qualifier(&self) -> Option<&str> {
        self.qualifier_start
            .map(|start| &self.spelling.as_ref()[start..])
    }

    #[cfg(test)]
    pub(crate) fn into_spelling(self) -> S {
        self.spelling
    }
}

fn canonical_parts(name: &str) -> Option<(u32, Option<usize>)> {
    let tail = name.strip_prefix('p')?;
    let digit_count = tail.bytes().take_while(u8::is_ascii_digit).count();
    if digit_count == 0 {
        return None;
    }
    let index = tail[..digit_count].parse().ok()?;
    match &tail[digit_count..] {
        "" => Some((index, None)),
        suffix => {
            let qualifier = suffix.strip_prefix('_').filter(|qualifier| {
                !qualifier.is_empty()
                    && qualifier
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            })?;
            Some((index, Some(name.len() - qualifier.len())))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ParameterName;

    #[test]
    fn parameter_name_immutable_storage_retains_the_checked_parts() {
        let text = "p12_face_A";
        let borrowed = ParameterName::<_, u32>::parse(text).unwrap();
        let owned = ParameterName::<_, u32>::parse(text.to_owned()).unwrap();
        for _ in 0..3 {
            assert_eq!(borrowed.as_str(), text);
            assert_eq!(owned.as_str(), text);
            assert_eq!(
                (borrowed.index(), borrowed.qualifier()),
                (12, Some("face_A"))
            );
            assert_eq!((owned.index(), owned.qualifier()), (12, Some("face_A")));
        }
    }
}

// SPDX-License-Identifier: Apache-2.0
//! Fallible traversal of STEP entity references.

use cadmpeg_core::decode::{DecodeContext, DepthGuard};
use cadmpeg_core::CodecError;

use crate::parse::Value;

struct Frame<'a, 'ctx> {
    value: &'a Value,
    next_child: usize,
    /// Keeps caller depth admission live until this value is left.
    _depth: DepthGuard<'ctx>,
}

/// Each frame holds one session depth level, so the session's recursion
/// ceiling bounds the frame stack.
pub(super) struct References<'a, 'ctx, 'arena> {
    frames: Vec<Frame<'a, 'ctx>>,
    root: Option<&'a Value>,
    ctx: &'ctx DecodeContext<'arena>,
}

pub(super) fn references<'a, 'ctx, 'arena>(
    value: &'a Value,
    ctx: &'ctx DecodeContext<'arena>,
) -> References<'a, 'ctx, 'arena> {
    References {
        frames: Vec::new(),
        root: Some(value),
        ctx,
    }
}

impl<'a> References<'a, '_, '_> {
    fn push_frame(&mut self, value: &'a Value) -> Result<(), CodecError> {
        let depth = self.ctx.enter_nested("step_reference_value_walk")?;
        self.frames.push(Frame {
            value,
            next_child: 0,
            _depth: depth,
        });
        Ok(())
    }

    fn advance(&mut self) -> Result<Option<u64>, CodecError> {
        if let Some(root) = self.root.take() {
            self.push_frame(root)?;
        }
        while let Some(frame) = self.frames.last_mut() {
            let child = match frame.value {
                Value::Reference(id) => {
                    let id = *id;
                    self.frames.pop();
                    return Ok(Some(id));
                }
                Value::List(values) => {
                    let child = values.get(frame.next_child);
                    if child.is_some() {
                        frame.next_child += 1;
                    }
                    child
                }
                Value::Typed(_, value) if frame.next_child == 0 => {
                    frame.next_child = 1;
                    Some(value.as_ref())
                }
                _ => None,
            };
            if let Some(child) = child {
                self.push_frame(child)?;
            } else {
                self.frames.pop();
            }
        }
        Ok(None)
    }
}

impl Iterator for References<'_, '_, '_> {
    type Item = Result<u64, CodecError>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.advance() {
            Ok(Some(id)) => Some(Ok(id)),
            Ok(None) => None,
            Err(error) => {
                self.root = None;
                self.frames.clear();
                Some(Err(error))
            }
        }
    }
}

pub(super) fn first_matching<'a>(
    values: impl IntoIterator<Item = &'a Value>,
    ctx: &DecodeContext<'_>,
    mut predicate: impl FnMut(u64) -> bool,
) -> Result<Option<u64>, CodecError> {
    for value in values {
        for reference in references(value, ctx) {
            let id = reference?;

            if predicate(id) {
                return Ok(Some(id));
            }
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::{first_matching, references};
    use crate::parse::Value;
    use crate::test_support::{with_policy_context, with_service_context};
    use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    #[test]
    fn nested_references_keep_source_order_without_a_collection() {
        let value = Value::List(vec![
            Value::Reference(1),
            Value::Typed(
                "WRAPPER".into(),
                Box::new(Value::List(vec![Value::Reference(2), Value::Integer(3)])),
            ),
            Value::Reference(4),
        ]);
        with_service_context(b"", |_, ctx| {
            assert_eq!(
                references(&value, ctx)
                    .collect::<Result<Vec<_>, _>>()
                    .expect("admitted traversal"),
                [1, 2, 4]
            );
            assert_eq!(
                first_matching([&value], ctx, |id| id > 1).expect("matching traversal"),
                Some(2)
            );
        });
    }

    #[test]
    fn admitted_nested_references_reach_the_leaf() {
        let mut value = Value::Reference(9);
        for _ in 0..256 {
            value = Value::Typed("WRAPPER".into(), Box::new(value));
        }
        let mut policy = DecodePolicy::service();
        policy.limits.max_recursion_depth = 1024;
        with_policy_context(b"", &policy, |_, ctx| {
            assert_eq!(
                references(&value, ctx)
                    .collect::<Result<Vec<_>, _>>()
                    .expect("admitted traversal"),
                [9]
            );
        });
    }

    #[test]
    fn reference_iterator_depth_follows_the_session_past_513_levels() {
        let mut value = Value::Reference(9);
        for _ in 0..600 {
            value = Value::List(vec![value]);
        }
        let mut policy = DecodePolicy::service();
        policy.limits.max_recursion_depth = 601;
        with_policy_context(b"", &policy, |_, ctx| {
            assert_eq!(
                references(&value, ctx)
                    .collect::<Result<Vec<_>, _>>()
                    .expect("admitted traversal"),
                [9]
            );
        });
        policy.limits.max_recursion_depth = 600;
        with_policy_context(b"", &policy, |_, ctx| {
            assert!(matches!(first_matching([&value], ctx, |_| true),
                Err(CodecError::ResourceLimit(refusal)) if refusal.dimension == ResourceDimension::RecursionDepth));
        });
    }

    #[test]
    fn reference_matching_propagates_caller_depth_refusal() {
        let value = Value::List(vec![Value::Typed(
            "WRAPPER".into(),
            Box::new(Value::Reference(9)),
        )]);
        let mut policy = DecodePolicy::service();
        policy.limits.max_recursion_depth = 2;
        with_policy_context(b"", &policy, |_, ctx| {
            assert!(matches!(first_matching([&value], ctx, |_| true),
                Err(CodecError::ResourceLimit(refusal)) if refusal.dimension == ResourceDimension::RecursionDepth
                    && refusal.operation == "step_reference_value_walk"));
        });
    }

    #[test]
    fn early_matching_releases_reference_depth_guards() {
        let value = Value::List(vec![Value::Reference(9)]);
        let mut policy = DecodePolicy::service();
        policy.limits.max_recursion_depth = 2;
        with_policy_context(b"", &policy, |_, ctx| {
            assert_eq!(
                first_matching([&value], ctx, |_| true).expect("matching traversal"),
                Some(9)
            );
            let _first = ctx
                .enter_nested("test first released level")
                .expect("root guard was released");
            let _second = ctx
                .enter_nested("test second released level")
                .expect("child guard was released");
        });
    }
}

//! Element checks: the values a segment's elements parse to, and the diagnostics they raise.

use crate::diagnostic::Rule;
use crate::element::Element;
use crate::segment::Segment;
use crate::spec::{ElementDef, rejects_code};
use crate::value::{ColumnType, LengthRange, Parsed};

use super::Projector;
use super::fill::leaf_text;
use super::plan::ElementPlan;
/// One defined element (or component) of the current segment, checked.
#[derive(Debug, Clone, Copy)]
pub(super) struct Checked {
    pub(super) element: usize,
    pub(super) component: Option<usize>,
    /// The column type its definition maps to.
    pub(super) kind: ColumnType,
    /// The parsed value; `None` when missing or of the wrong type.
    pub(super) value: Option<Parsed>,
}

impl<'s> Projector<'s> {
    /// Checks every element the spec defines for the segment and keeps the
    /// parsed values of the ones a column reads.
    pub(super) fn check(
        &mut self,
        elements: &[ElementPlan<'_>],
        segment: &Segment<'_>,
        joined: &mut Vec<u8>,
    ) {
        self.checked.clear();
        for plan in elements {
            let (position, element) = (plan.position, plan.def);
            if plan.components.is_empty() {
                let text = leaf_text(segment, position, None, self.separator, joined);
                let value = self.check_value(segment, position, None, element, text, plan.read);
                if plan.read {
                    self.checked.push(Checked {
                        element: position,
                        component: None,
                        kind: plan.kind,
                        value,
                    });
                }
                continue;
            }
            let parts: &[std::borrow::Cow<'_, [u8]>] = match segment.element(position) {
                Some(Element::Composite(parts)) => parts,
                Some(Element::Simple(value)) => std::slice::from_ref(value),
                None => &[],
            };
            if parts.iter().all(|part| part.is_empty()) {
                if element.required {
                    self.report(
                        Rule::RequiredElementMissing {
                            segment_id: segment.id.to_vec(),
                            element: position,
                            component: None,
                            name: element.name.clone(),
                        },
                        segment.index,
                        position,
                        None,
                        &[],
                    );
                }
                continue;
            }
            let declared = plan.declared;
            if parts.len() > declared {
                let extra = parts.get(declared).map_or(&[][..], |part| part.as_ref());
                self.report(
                    Rule::CompositeShape {
                        segment_id: segment.id.to_vec(),
                        element: position,
                        name: element.name.clone(),
                        declared,
                        found: parts.len(),
                    },
                    segment.index,
                    position,
                    declared.checked_add(1),
                    extra,
                );
            }
            for &(component, def, kind, read) in &plan.components {
                let text = component
                    .checked_sub(1)
                    .and_then(|at| parts.get(at))
                    .map_or(&[][..], |part| part.as_ref());
                let value = self.check_value(segment, position, Some(component), def, text, read);
                if read {
                    self.checked.push(Checked {
                        element: position,
                        component: Some(component),
                        kind,
                        value,
                    });
                }
            }
        }
    }

    /// Checks one value against its definition, reports what fails, and
    /// returns the parsed value (`None` when missing or of the wrong type).
    /// When no column reads it (`keep` false) the value is only validated
    /// and `None` is returned; the diagnostics are the same.
    fn check_value(
        &mut self,
        segment: &Segment<'_>,
        element: usize,
        component: Option<usize>,
        def: &ElementDef,
        text: &[u8],
        keep: bool,
    ) -> Option<Parsed> {
        if text.is_empty() {
            if def.required {
                self.report(
                    Rule::RequiredElementMissing {
                        segment_id: segment.id.to_vec(),
                        element,
                        component,
                        name: def.name.clone(),
                    },
                    segment.index,
                    element,
                    component,
                    &[],
                );
            }
            return None;
        }
        let Some(value) = ColumnType::of(Some(def.kind)).parse(text) else {
            self.report(
                Rule::TypeMismatch {
                    segment_id: segment.id.to_vec(),
                    element,
                    component,
                    name: def.name.clone(),
                    expected: def.kind,
                },
                segment.index,
                element,
                component,
                text,
            );
            return None;
        };
        let value = keep.then_some(value);
        // Numeric lengths count digits only, as X12 does: no sign, no point.
        if !def.kind.length_in_range(
            text,
            LengthRange {
                min: def.min,
                max: def.max,
            },
        ) {
            self.report(
                Rule::LengthOutOfRange {
                    segment_id: segment.id.to_vec(),
                    element,
                    component,
                    name: def.name.clone(),
                    min: def.min,
                    max: def.max,
                    length: def.kind.length_of(text),
                },
                segment.index,
                element,
                component,
                text,
            );
        } else {
            // The occurrence the segment matched may list its own codes for
            // the element; otherwise the element's own list applies.
            let own = self.occurrence.and_then(|occurrence| {
                occurrence
                    .codes
                    .get(&(element, component))
                    .map(|codes| (codes, occurrence))
            });
            let codes = own.map_or(&def.codes, |(codes, _)| codes);
            // Every code fits the element's lengths, so a value of the wrong
            // length is reported once, as a length.
            if !rejects_code(codes, text) {
                return value;
            }
            self.report(
                Rule::CodeNotInList {
                    segment_id: segment.id.to_vec(),
                    element,
                    component,
                    name: def.name.clone(),
                    codes: codes.clone(),
                    occurrence: own.map(|(_, occurrence)| occurrence.name.clone()),
                },
                segment.index,
                element,
                component,
                text,
            );
        }
        value
    }
}

//! Construction of a [`Spec`] from JSON.
//!
//! A spec compiles as a pipeline: `STEPS` runs in order over a `Build`, one
//! step per section or structural check, each reading what the steps before
//! it built.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value;

use super::Spec;
use super::balance::compile_balancing;
use super::build_loops::{check_cycles, compile_loop, held_segments};
use super::compile::compile_tables;
use super::error::SpecError;
use super::loops::{LoopId, Trigger};
use super::occurrences::{check_trigger, compile_occurrences};
use super::raw::{RawLoop, RawSegment, RawSpec, RawVersion};
use super::render::render_trigger;
use super::segments::{SegmentDef, compile_elements};
use super::shape::{check_shape, section};
use super::summaries::{compile_default_table, compile_summaries};
use super::version::compile_version;

/// A spec while it compiles, with the parts of the JSON the later steps read.
struct Build {
    spec: Spec,
    /// The loops as written, in spec order.
    defs: Vec<(String, RawLoop)>,
    version: Option<RawVersion>,
    default_table: Option<String>,
}

type Step = fn(&mut Build) -> Result<(), SpecError>;

/// The compile steps, in the order they run.
const STEPS: [Step; 11] = [
    read_loops,
    compile_loop_headers,
    link_loops,
    compile_segment_section,
    compile_loop_occurrences,
    compile_version_section,
    check_trigger_ambiguity,
    compile_table_section,
    compile_balancing_section,
    compile_summary_section,
    compile_default_table_section,
];

impl Spec {
    pub(crate) fn from_value(source: Value) -> Result<Spec, SpecError> {
        check_shape(&source)?;
        let mut build = Build::new(source)?;
        STEPS.iter().try_for_each(|step| step(&mut build))?;
        Ok(build.spec)
    }

    fn check_ambiguity(&self) -> Result<(), SpecError> {
        let groups = std::iter::once(self.roots.as_slice())
            .chain(self.loops.iter().map(|def| def.children.as_slice()));
        for siblings in groups {
            for (i, &first) in siblings.iter().enumerate() {
                for &second in &siblings[i + 1..] {
                    let trigger = &self.loops[first.0].trigger;
                    let other = &self.loops[second.0].trigger;
                    if trigger.segment != other.segment {
                        continue;
                    }
                    let text = |bytes: &[u8]| String::from_utf8_lossy(bytes).into_owned();
                    let parent = self.loops[first.0]
                        .parent
                        .map(|parent| self.loops[parent.0].name.clone());
                    if trigger == other {
                        return Err(SpecError::AmbiguousTrigger {
                            first: self.loops[first.0].name.clone(),
                            second: self.loops[second.0].name.clone(),
                            parent,
                            segment: text(&trigger.segment),
                            conditions: trigger
                                .conditions
                                .iter()
                                .map(|(position, value)| (*position, text(value)))
                                .collect(),
                        });
                    }
                    // Siblings are told apart when a shared position requires
                    // different values, or when one trigger's conditions
                    // contain the other's: the engine then prefers the one
                    // with more conditions and the other is the catch-all.
                    let excluded = trigger.conditions.iter().any(|(position, value)| {
                        other
                            .conditions
                            .iter()
                            .any(|(p, v)| p == position && v != value)
                    });
                    let contains = |big: &Trigger, small: &Trigger| {
                        small
                            .conditions
                            .iter()
                            .all(|condition| big.conditions.contains(condition))
                    };
                    let nested = contains(trigger, other) || contains(other, trigger);
                    if !excluded && !nested {
                        return Err(SpecError::OverlappingTriggers {
                            parent,
                            a: self.loops[first.0].name.clone(),
                            b: self.loops[second.0].name.clone(),
                            conditions_a: render_trigger(trigger),
                            conditions_b: render_trigger(other),
                        });
                    }
                }
            }
        }
        Ok(())
    }
}

impl Build {
    /// The spec's name and an empty structure, or why the top level is invalid.
    fn new(source: Value) -> Result<Build, SpecError> {
        let raw = RawSpec::deserialize(&source).map_err(|e| SpecError::Schema {
            loop_name: None,
            source: e,
        })?;
        if raw.loops.is_empty() {
            return Err(SpecError::NoLoops {
                spec_name: raw.name,
            });
        }
        Ok(Build {
            spec: Spec {
                name: raw.name,
                loops: Vec::new(),
                roots: Vec::new(),
                segments: BTreeMap::new(),
                tables: Vec::new(),
                balancing: Vec::new(),
                summaries: Vec::new(),
                default_table: None,
                version: None,
                source,
            },
            defs: Vec::new(),
            version: raw.version,
            default_table: raw.default_table,
        })
    }
}

fn read_loops(build: &mut Build) -> Result<(), SpecError> {
    build.defs = section(&build.spec.source, "loops")
        .into_iter()
        .map(|(name, value)| {
            RawLoop::deserialize(value)
                .map(|def| (name.to_string(), def))
                .map_err(|e| SpecError::Schema {
                    loop_name: Some(name.to_string()),
                    source: e,
                })
        })
        .collect::<Result<_, _>>()?;
    Ok(())
}

fn compile_loop_headers(build: &mut Build) -> Result<(), SpecError> {
    let ids: BTreeMap<&str, LoopId> = build
        .defs
        .iter()
        .enumerate()
        .map(|(index, (name, _))| (name.as_str(), LoopId(index)))
        .collect();
    let loops = build
        .defs
        .iter()
        .map(|(name, def)| compile_loop(name, def, &ids))
        .collect::<Result<Vec<_>, _>>()?;
    check_cycles(&loops)?;
    build.spec.loops = loops;
    Ok(())
}

/// Gives each loop its children and the spec its top-level loops.
fn link_loops(build: &mut Build) -> Result<(), SpecError> {
    let spec = &mut build.spec;
    let parents: Vec<Option<LoopId>> = spec.loops.iter().map(|def| def.parent).collect();
    for (index, parent) in parents.into_iter().enumerate() {
        match parent {
            Some(parent) => spec.loops[parent.0].children.push(LoopId(index)),
            None => spec.roots.push(LoopId(index)),
        }
    }
    Ok(())
}

fn compile_segment_section(build: &mut Build) -> Result<(), SpecError> {
    build.spec.segments = section(&build.spec.source, "segments")
        .into_iter()
        .map(|(id, value)| compile_segment(id, value))
        .collect::<Result<_, _>>()?;
    Ok(())
}

fn compile_segment(id: &str, value: &Value) -> Result<(Vec<u8>, SegmentDef), SpecError> {
    if id.is_empty() {
        return Err(SpecError::EmptySegmentId {
            loop_name: None,
            key: "segments.\"\"".into(),
        });
    }
    let def = RawSegment::deserialize(value).map_err(|e| SpecError::SegmentSchema {
        segment: id.to_string(),
        source: e,
    })?;
    let elements = compile_elements(id, &def.elements, None)?;
    Ok((id.as_bytes().to_vec(), SegmentDef { elements }))
}

fn compile_loop_occurrences(build: &mut Build) -> Result<(), SpecError> {
    let spec = &mut build.spec;
    for ((name, def), compiled) in build.defs.iter().zip(spec.loops.iter_mut()) {
        compiled.occurrences = compile_occurrences(name, &def.occurrences, &spec.segments)?;
        check_trigger(name, &compiled.occurrences, &compiled.trigger)?;
        compiled.segments = held_segments(compiled);
    }
    Ok(())
}

fn compile_version_section(build: &mut Build) -> Result<(), SpecError> {
    build.spec.version = build.version.as_ref().map(compile_version).transpose()?;
    Ok(())
}

fn check_trigger_ambiguity(build: &mut Build) -> Result<(), SpecError> {
    build.spec.check_ambiguity()
}

fn compile_table_section(build: &mut Build) -> Result<(), SpecError> {
    let spec = &build.spec;
    build.spec.tables = compile_tables(spec, section(&spec.source, "tables"))?;
    Ok(())
}

fn compile_balancing_section(build: &mut Build) -> Result<(), SpecError> {
    let spec = &build.spec;
    build.spec.balancing = compile_balancing(spec, section(&spec.source, "balancing"))?;
    Ok(())
}

fn compile_summary_section(build: &mut Build) -> Result<(), SpecError> {
    let spec = &build.spec;
    build.spec.summaries = compile_summaries(spec, section(&spec.source, "summaries"))?;
    Ok(())
}

fn compile_default_table_section(build: &mut Build) -> Result<(), SpecError> {
    build.spec.default_table = compile_default_table(&build.spec, build.default_table.take())?;
    Ok(())
}

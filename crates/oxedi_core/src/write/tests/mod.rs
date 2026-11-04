//! Tests of the write plan (`plan.rs`), of the text of the writer's
//! findings, origins and errors (`display.rs`), of the origin a
//! read-back diagnostic gets (`origin.rs`) and of a cell's element text
//! (`render.rs`). The writer itself is tested
//! on parsed files in the crate's `tests/write_*.rs`.

mod display;
mod envelope;
mod origin;
mod plan;
mod render;

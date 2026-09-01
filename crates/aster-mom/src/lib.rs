#![forbid(unsafe_code)]
//! MoM (Model Manifest) engine: parse `mom.yaml`, resolve model entries
//! against a catalog, and run the per-turn switch evaluation (spec 7.1).

mod catalog;
mod eval;
mod manifest;
mod resolve;

pub use catalog::{Catalog, CatalogModel};
pub use eval::{Engine, Fired, Selection, Signals, SwitchRecord};
pub use manifest::{
    Condition, Manifest, MemoryBand, ModelEntry, Power, Rule, Thinking, discover, load, parse,
};
pub use resolve::{Access, Resolution, Resolver};

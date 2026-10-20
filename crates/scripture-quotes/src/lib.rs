#![doc = include_str!("../README.md")]

mod align;
mod engine;
mod index;
mod near;
mod scope;
mod search;
pub mod text;
mod transcript;

pub use engine::{Engine, EngineBuilder, Translation, TranslationId, VerseRef};
pub use scope::{Scope, ScopeError};
pub use search::{AlsoIn, Found, Kind, Options, Quotation, Run, Searcher};
pub use topos_bible;
pub use transcript::Transcript;

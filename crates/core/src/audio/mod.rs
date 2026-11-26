//! Playback: decoding with symphonia through rodio, out through cpal.

mod engine;
mod remote;

pub use engine::{Engine, EngineEvent, EngineStatus, Media, Output};
pub use remote::{RangeSet, Remote, RemoteWatcher};

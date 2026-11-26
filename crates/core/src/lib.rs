//! Everything the Cpz Music apps share that is not drawing pixels: talking to the
//! server, signing in, the queue, and what the device keeps offline.

pub mod api;
pub mod audio;
pub mod auth;
pub mod downloads;
pub mod http;
pub mod listens;
pub mod player;
pub mod playlists;
pub mod queue;
pub mod store;
pub mod types;

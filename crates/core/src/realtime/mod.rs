//! Several devices sharing one player over Supabase Realtime.

mod channel;
mod devices;

pub use channel::{Channel, ChannelConfig, ChannelEvent, websocket_url};
pub use devices::{
    DevicePresence, Hub, HubState, Identity, PLAYER_TOPIC, SharedState, elect_active,
};

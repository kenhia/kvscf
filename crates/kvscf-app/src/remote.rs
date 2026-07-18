//! Remote channel to kdeskdash — feature `remote` (WI #471).
//!
//! Slice 1 (current): placeholder only, so the feature and the two-artifact build exist from
//! day one. The outbound WebSocket client (connect over Tailscale, push the instance list,
//! receive `{ "select": <hwnd> }` → focus) lands in a later slice, entirely inside this module.
//! A `kvscf-local` build compiles with this module absent — zero comms code.

#![allow(dead_code)]

/// Placeholder for the channel handle the app will own once the client is built.
pub struct Channel;

impl Channel {
    /// Will connect to kdeskdash and start pushing/receiving. No-op for now.
    pub fn start() -> Option<Channel> {
        None
    }
}

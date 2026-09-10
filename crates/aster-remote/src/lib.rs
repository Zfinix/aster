//! Remote control for the Aster agent over messaging channels. Each adapter turns
//! inbound chat messages into headless `aster chat --stream` turns and relays
//! approval prompts back as tappable buttons.

mod bridge;
mod channel;
mod imessage;
mod markdown;
mod mcp_server;
mod photon;
mod telegram;

pub use bridge::{Answer, Turn, TurnEvent, TurnOutcome, WireMessage, ask_once, run_turn};
pub use imessage::{IMessageConfig, run_imessage};
pub use mcp_server::run_mcp_telegram;
pub use photon::{PhotonConfig, run_photon};
pub use telegram::{TelegramConfig, run_telegram};

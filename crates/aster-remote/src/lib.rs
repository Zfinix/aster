//! Remote control for the Aster agent over messaging channels.
//!
//! Each adapter turns inbound chat messages into headless `aster chat --stream`
//! turns and relays approval prompts back as tappable buttons.

mod bridge;
mod markdown;
mod mcp_server;
mod telegram;

pub use bridge::{Answer, Turn, TurnEvent, TurnOutcome, WireMessage, run_turn};
pub use mcp_server::run_mcp_telegram;
pub use telegram::{TelegramConfig, run_telegram};

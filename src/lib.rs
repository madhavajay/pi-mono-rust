pub mod agent;
pub mod ai;
pub mod api;
pub mod cli;
pub mod coding_agent;
pub mod config;
pub mod core;
pub mod modes;
pub mod rpc;
pub mod test_port;
pub mod tools;
pub mod tui;

#[cfg(feature = "python")]
pub mod python;

pub use cli::args::*;
pub use core::compaction::*;
pub use core::messages::*;
pub use core::session_manager::*;

// Re-export the Python module initialization function when the python feature is enabled
#[cfg(feature = "python")]
pub use python::_pi_mono;

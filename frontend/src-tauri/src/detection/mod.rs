//! Automatic meeting detection.
//!
//! Split deliberately: [`policy`] holds the entire rule set as pure, clock-free
//! functions that unit tests can exercise without hardware, while [`service`]
//! does the impure work of reading settings, talking to Core Audio and starting
//! a recording.

pub mod commands;
pub mod policy;
pub mod service;

pub use policy::{DetectionConfig, Decision};

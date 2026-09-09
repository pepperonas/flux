//! Pure logic: no audio, no interface, no I/O.
//!
//! Nothing in this module may depend on `eframe`, `egui`, `cpal` or any crate
//! that touches the outside world. That constraint is what keeps the test suite
//! fast and what lets every musical rule be asserted directly.

pub mod ids;
pub mod music;
pub mod quantize;
pub mod transport;

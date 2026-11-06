//! Noisy Skylines - Terrain Height generator with SIMD optimizations, CPU only!
//!
//! Performance tests on a Ryzen 9 5900X:
//! Vec2 -> f32 : 20.42 M samples/s
//! Vec2 -> f64 : 4.74 M samples/s
//! DVec2 -> f32 : 17.90 M samples/s
//! DVec2 -> f64 : 4.81 M samples/s
//!
//! Example usage:
//! ```
//! use crate::compiler::{GenWorkspace, Program};
//! use crate::presets::NoiseConfig;
//!
//! pub fn execute_terrain(
//!     preset: &CompiledPreset,
//!     points: &[DVec2]            // Either DVec2 or Vec2 inputs
//!     output: &mut Vec<f32>,         // Either f32 or f64 outputs
//! ) {
//!     println!(
//!         "[Terrain] Selected execution backend: {:?}",
//!         GenBackend::detected()
//!     );
//!
//!     // Automatically chooses how to generate terrain heights by looking at whether it is supposed to make f32 or f64, or support large coordinates using DVec2
//!     output.append(&mut GenWorkspace::execute_muted(
//!         preset,
//!         points,
//!         None                    // Explicit Execution backend selection, or None for Automatic selection
//!     ));
//! }
//! ```
//! Sorry, more info to come!

pub mod compiler;
pub mod presets;
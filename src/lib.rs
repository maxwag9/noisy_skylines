//! Noisy Skylines - Terrain Height generator with SIMD optimizations, CPU only!
//!
//! Example usage:
//! ```
//! pub fn execute_terrain(
//!     program: &Program,
//!     noises: &[NoiseConfig],
//!     xs: &[f32],
//!     zs: &[f32],
//!     output: &mut [f32],
//! ) -> GenWorkspace {
//!     let mut workspace = GenWorkspace::new(&program);
//!
//!     println!(
//!         "[Terrain] Selected execution backend: {:?}",
//!         workspace.backend()
//!     );
//!
//!     workspace.execute(
//!         &program,
//!         &noises,
//!         &xs,
//!         &zs,
//!         output
//!!     );
//!     workspace
//! }
//! ```
//! Sorry, more info to come!

pub mod compiler;
pub mod presets;
use glam::{DVec2, Vec2};
use noisy_skylines::compiler::{GenBackend, GenWorkspace, compile_expression, instruction_count};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

use noisy_skylines::presets::{CompiledPreset, NoiseConfig, compile_preset, fbm_dvec2_f32, fbm_scalar, noise2d};

fn continental_preset() -> CompiledPreset {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("presets")
        .join("continental.yaml");

    compile_preset(path).expect("Failed to compile continental.yaml")
}

fn generate_coordinates(count: usize) -> Vec<Vec2> {
    (0..count)
        .map(|i| {
            Vec2::new(
                ((i * 37 % 1021) as f32 - 510.0) * 1.7,
                ((i * 97 % 997) as f32 - 498.0) * 1.3,
            )
        })
        .collect()
}
fn generate_coordinates_d(count: usize) -> Vec<DVec2> {
    (0..count)
        .map(|i| {
            DVec2::new(
                ((i * 37 % 1021) as f64 - 510.0) * 1.7,
                ((i * 97 % 997) as f64 - 498.0) * 1.3,
            )
        })
        .collect()
}

fn assert_close(actual: &[f32], expected: &[f32]) {
    assert_eq!(actual.len(), expected.len());

    for (index, (&a, &b)) in actual.iter().zip(expected).enumerate() {
        let tolerance = 0.0002 + 0.0002 * a.abs().max(b.abs());

        assert!(
            a.is_finite() && b.is_finite() && (a - b).abs() <= tolerance,
            "Mismatch at {index}: actual={a}, expected={b}, difference={}, tolerance={tolerance}",
            (a - b).abs()
        );
    }
}


#[test]
fn continental_preset_compiles() {
    let preset = continental_preset();

    assert!(!preset.name.is_empty());
    assert!(!preset.noises.is_empty());
    assert!(preset.noise_ids.contains_key("continents"));
    assert!(preset.noise_ids.contains_key("mountains"));

    assert!(preset.program.register_count > 2);
    assert!(
        (preset.program.output as usize) < preset.program.register_count
    );

    assert!(instruction_count(&preset.program) > 2);

    assert!(!preset.expression.is_empty());
}

#[test]
fn default_preset_compiles() {
    let preset = CompiledPreset::default();

    assert!(!preset.noises.is_empty());
    assert!(!preset.program.instructions.is_empty());

    assert!(preset.noise_ids.contains_key("continents"));
    assert!(preset.noise_ids.contains_key("mountains"));
}

#[test]
fn noise_is_deterministic() {
    let a = noise2d(Vec2::new(12.345, -67.891), 1741);
    let b = noise2d(Vec2::new(12.345, -67.891), 1741);

    assert_eq!(a.to_bits(), b.to_bits());
}

#[test]
fn noise_changes_with_seed() {
    let mut different = false;

    for i in 0..100 {
        let x = i as f32 * 0.371;
        let z = i as f32 * -0.127;

        let a = noise2d(Vec2::new(x, z), 1741);
        let b = noise2d(Vec2::new(x, z), 8197);

        if a.to_bits() != b.to_bits() {
            different = true;
            break;
        }
    }

    assert!(different, "Different seeds produced identical noise everywhere");
}

#[test]
fn noise_is_zero_at_lattice_points() {
    for x in -20..=20 {
        for z in -20..=20 {
            let value = noise2d(Vec2::new(x as f32, z as f32), 12345);

            assert!(
                value.abs() < 1.0e-6,
                "Unexpected lattice value at ({x}, {z}): {value}"
            );
        }
    }
}

#[test]
fn noise_is_continuous_at_cell_boundaries() {
    for z in [-7.31, -1.27, 0.43, 4.91, 12.37] {
        let left = noise2d(Vec2::new(3.0 - 0.0001, z), 12345);
        let right = noise2d(Vec2::new(3.0 + 0.0001, z), 12345);

        assert!(
            (left - right).abs() < 0.01,
            "Discontinuity at x=3, z={z}: left={left}, right={right}"
        );
    }
}

#[test]
fn fbm_is_deterministic_and_finite() {
    let preset = continental_preset();
    let config = &preset.noises[0];

    let first = fbm_scalar(config, Vec2::new(123.45, -678.91));
    let second = fbm_scalar(config, Vec2::new(123.45, -678.91));

    assert_eq!(first.to_bits(), second.to_bits());
    assert!(first.is_finite());

    let zero_octaves = NoiseConfig {
        seed: 123,
        frequency: 0.005,
        octaves: 0,
        lacunarity: 2.0,
        gain: 0.5,
    };

    assert_eq!(fbm_scalar(&zero_octaves, Vec2::new(100.0, 100.0)), 0.0);
}

#[test]
fn dvec2_f32_preserves_large_world_coordinate_precision() {
    let config = NoiseConfig {
        seed: 1741,
        frequency: 0.005,
        octaves: 5,
        lacunarity: 2.0,
        gain: 0.5,
    };

    let a = DVec2::new(
        10_000_000_000.0,
        -4_000_000_000.0,
    );

    let b = DVec2::new(
        10_000_000_000.25,
        -4_000_000_000.0,
    );

    assert_eq!(a.x as f32, b.x as f32);

    let height_a = fbm_dvec2_f32(&config, a).unwrap();
    let height_b = fbm_dvec2_f32(&config, b).unwrap();

    assert_ne!(
        height_a.to_bits(),
        height_b.to_bits(),
        "Large-coordinate precision was lost"
    );
}
#[test]
fn all_precision_combinations_execute() {
    let preset = continental_preset();

    let points32 = vec![
        Vec2::new(100.0, 200.0),
        Vec2::new(150.0, 250.0),
        Vec2::new(-300.0, 400.0),
    ];

    let points64 = vec![
        DVec2::new(100.123456789, 200.987654321),
        DVec2::new(150.123456789, 250.987654321),
        DVec2::new(-300.123456789, 400.987654321),
    ];

    let mut output32 = vec![0.0_f32; 3];
    let mut output64 = vec![0.0_f64; 3];

    output32.append(&mut GenWorkspace::execute_muted(&preset, points32.as_slice(), Some(GenBackend::Scalar)));
    output64.append(&mut GenWorkspace::execute_muted(&preset, points32.as_slice(), Some(GenBackend::Scalar)));
    output32.append(&mut GenWorkspace::execute_muted(&preset, points64.as_slice(), Some(GenBackend::Scalar)));
    output64.append(&mut GenWorkspace::execute_muted(&preset, points64.as_slice(), Some(GenBackend::Scalar)));

    assert!(output32.iter().all(|v| v.is_finite()));
    assert!(output64.iter().all(|v| v.is_finite()));
}

#[test]
fn expression_compiler_rejects_invalid_expressions() {
    let invalid = [
        "x +",
        "foo(x)",
        "fbm(missing, x, z)",
        "fbm(noise, x)",
        "abs(x, z)",
        "y + 1",
        "1 @ 2",
    ];

    for expression in invalid {
        assert!(
            compile_expression(expression, &BTreeMap::new()).is_err(),
            "Expression unexpectedly compiled: {expression}"
        );
    }
}

#[test]
fn continental_terrain_is_finite_and_non_flat() {
    let preset = continental_preset();

    let side = 65;
    let count = side * side;

    let mut points = Vec::with_capacity(count);

    for z in 0..side {
        for x in 0..side {
            points.push(Vec2::new((x as f32 - 32.0) * 32.0, (z as f32 - 32.0) * 32.0));
        }
    }

    let mut output = vec![0.0f32; count];
    output.append(&mut GenWorkspace::execute_muted(&preset, points.as_slice(), Some(GenBackend::Scalar)));

    assert!(output.iter().all(|v| v.is_finite()));

    let min = output.iter().copied().fold(f32::INFINITY, f32::min);
    let max = output.iter().copied().fold(f32::NEG_INFINITY, f32::max);

    assert!(max - min > 0.01, "Generated terrain is effectively flat");

    println!(
        "Continental terrain: min={min}, max={max}, range={}",
        max - min
    );
}

#[test]
#[ignore = "Manual performance benchmark. Run with cargo test --release -- --ignored --nocapture"]
fn benchmark_continental_generation() {
    let preset = CompiledPreset::default();

    let side = 256;
    let count = side * side;
    let iterations = 8;

    let points = generate_coordinates(count);

    let mut scalar_output = vec![0.0; count];
    let mut automatic_output = vec![0.0; count];

    let scalar_start = Instant::now();

    for _ in 0..iterations {
        scalar_output.append(&mut GenWorkspace::execute_muted(&preset, points.as_slice(), Some(GenBackend::Scalar)));
    }

    let scalar_elapsed = scalar_start.elapsed();

    let automatic_start = Instant::now();

    for _ in 0..iterations {
        automatic_output.append(&mut GenWorkspace::execute_muted(&preset, points.as_slice(), None));
    }

    let automatic_elapsed = automatic_start.elapsed();

    std::hint::black_box(
        scalar_output.iter().copied().sum::<f32>(),
    );

    std::hint::black_box(
        automatic_output.iter().copied().sum::<f32>(),
    );

    assert_close(&automatic_output, &scalar_output);

    let total_samples = (count * iterations) as f64;

    let scalar_rate =
        total_samples / scalar_elapsed.as_secs_f64() / 1_000_000.0;

    let automatic_rate =
        total_samples / automatic_elapsed.as_secs_f64() / 1_000_000.0;

    let speedup = scalar_elapsed.as_secs_f64()
        / automatic_elapsed.as_secs_f64();

    println!();
    println!("========== NOISY SKYLINES BENCHMARK ==========");
    println!("Automatic Backend:   {:?}", GenBackend::detected());
    println!("Samples per pass:    {count}");
    println!("Iterations:          {iterations}");
    println!("Scalar time:         {scalar_elapsed:?}");
    println!("Automatic time:      {automatic_elapsed:?}");
    println!("Scalar throughput:   {scalar_rate:.2} M samples/s");
    println!("Automatic throughput:{automatic_rate:.2} M samples/s");
    println!("Speedup:             {speedup:.2}x");
    println!("==============================================");
}

#[test]
#[ignore = "Manual performance benchmark. Run with cargo test --release -- --ignored --nocapture"]
fn benchmark_continental_generation_f64() {
    let preset = CompiledPreset::default();

    let side = 256;
    let count = side * side;
    let iterations = 8;

    let points = generate_coordinates(count);

    let mut f32_output: Vec<f32> = vec![0.0; count];
    let mut f64_output: Vec<f64> = vec![0.0; count];

    let f32_start = Instant::now();

    for _ in 0..iterations {
        f32_output.append(&mut GenWorkspace::execute_muted(&preset, points.as_slice(), None));
    }

    let f32_elapsed = f32_start.elapsed();

    let f64_start = Instant::now();

    for _ in 0..iterations {
        f64_output.append(&mut GenWorkspace::execute_muted(&preset, points.as_slice(), None));
    }

    let f64_elapsed = f64_start.elapsed();

    std::hint::black_box(
        f32_output.iter().copied().sum::<f32>(),
    );

    std::hint::black_box(
        f64_output.iter().copied().sum::<f64>(),
    );

    let total_samples = (count * iterations) as f64;

    let f32_rate =
        total_samples / f32_elapsed.as_secs_f64() / 1_000_000.0;

    let f64_rate =
        total_samples / f64_elapsed.as_secs_f64() / 1_000_000.0;

    let speedup = f64_elapsed.as_secs_f64()
        / f32_elapsed.as_secs_f64();

    println!();
    println!("========== NOISY SKYLINES BENCHMARK - Vec2 (f32) into F32 vs F64 automatic ==========");
    println!("Backend:             {}", GenBackend::detected());
    println!("Samples per pass:    {count}");
    println!("Iterations:          {iterations}");
    println!("F32 time:         {f32_elapsed:?}");
    println!("F64 time:      {f64_elapsed:?}");
    println!("F32 throughput:   {f32_rate:.2} M samples/s");
    println!("F64 throughput:{f64_rate:.2} M samples/s");
    println!("Speedup:             {speedup:.2}x");
    println!("==============================================");
}
#[test]
#[ignore = "Manual performance benchmark. Run with cargo test --release -- --ignored --nocapture"]
fn benchmark_continental_generation_f64_dvec() {
    let preset = CompiledPreset::default();

    let side = 256;
    let count = side * side;
    let iterations = 8;

    let points = generate_coordinates_d(count);

    let mut f32_output: Vec<f32> = vec![0.0; count];
    let mut f64_output: Vec<f64> = vec![0.0; count];

    let f32_start = Instant::now();

    for _ in 0..iterations {
        f32_output.append(&mut GenWorkspace::execute_muted::<_, f32>(&preset, points.as_slice(), None));
    }

    let f32_elapsed = f32_start.elapsed();

    let f64_start = Instant::now();

    for _ in 0..iterations {
        f64_output.append(&mut GenWorkspace::execute_muted::<_, f64>(&preset, points.as_slice(), None));
    }

    let f64_elapsed = f64_start.elapsed();

    std::hint::black_box(
        f32_output.iter().copied().sum::<f32>(),
    );

    std::hint::black_box(
        f64_output.iter().copied().sum::<f64>(),
    );

    let total_samples = (count * iterations) as f64;

    let f32_rate =
        total_samples / f32_elapsed.as_secs_f64() / 1_000_000.0;

    let f64_rate =
        total_samples / f64_elapsed.as_secs_f64() / 1_000_000.0;

    let speedup = f64_elapsed.as_secs_f64()
        / f32_elapsed.as_secs_f64();

    println!();
    println!("========== NOISY SKYLINES BENCHMARK - DVec2 (f64) into F32 vs F64 automatic ==========");
    println!("Backend:             {}", GenBackend::detected());
    println!("Samples per pass:    {count}");
    println!("Iterations:          {iterations}");
    println!("F32 time:         {f32_elapsed:?}");
    println!("F64 time:      {f64_elapsed:?}");
    println!("F32 throughput:   {f32_rate:.2} M samples/s");
    println!("F64 throughput:{f64_rate:.2} M samples/s");
    println!("Speedup:             {speedup:.2}x");
    println!("==============================================");
}
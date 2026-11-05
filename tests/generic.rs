use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

use noisy_skylines::compiler::{
    compile_expression,
    execute_scalar,
    execute_scalar_with_workspace,
    execute_avx2,
    instruction_count,
    GenBackend,
    GenWorkspace,
    ScalarWorkspace,
};

use noisy_skylines::presets::{
    compile_preset,
    noise2d,
    fbm_scalar,
    CompiledPreset,
    NoiseConfig,
};

fn continental_preset() -> CompiledPreset {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("presets")
        .join("continental.yaml");

    compile_preset(path).expect("Failed to compile continental.yaml")
}

fn generate_coordinates(count: usize) -> (Vec<f32>, Vec<f32>) {
    let xs = (0..count)
        .map(|i| ((i * 37 % 1021) as f32 - 510.0) * 1.7)
        .collect();

    let zs = (0..count)
        .map(|i| ((i * 97 % 997) as f32 - 498.0) * 1.3)
        .collect();

    (xs, zs)
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
    let a = noise2d(12.345, -67.891, 1741);
    let b = noise2d(12.345, -67.891, 1741);

    assert_eq!(a.to_bits(), b.to_bits());
}

#[test]
fn noise_changes_with_seed() {
    let mut different = false;

    for i in 0..100 {
        let x = i as f32 * 0.371;
        let z = i as f32 * -0.127;

        let a = noise2d(x, z, 1741);
        let b = noise2d(x, z, 8197);

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
            let value = noise2d(x as f32, z as f32, 12345);

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
        let left = noise2d(3.0 - 0.0001, z, 12345);
        let right = noise2d(3.0 + 0.0001, z, 12345);

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

    let first = fbm_scalar(config, 123.45, -678.91);
    let second = fbm_scalar(config, 123.45, -678.91);

    assert_eq!(first.to_bits(), second.to_bits());
    assert!(first.is_finite());

    let zero_octaves = NoiseConfig {
        seed: 123,
        frequency: 0.005,
        octaves: 0,
        lacunarity: 2.0,
        gain: 0.5,
    };

    assert_eq!(fbm_scalar(&zero_octaves, 100.0, 100.0), 0.0);
}

#[test]
fn expression_arithmetic_respects_precedence() {
    let program = compile_expression(
        "2 + 3 * 4 - 8 / 2",
        &BTreeMap::new(),
    )
        .expect("Failed to compile arithmetic expression");

    let xs = [0.0, 1.0, 2.0];
    let zs = [0.0, 1.0, 2.0];

    let mut output = [0.0; 3];

    execute_scalar(
        &program,
        &[],
        &xs,
        &zs,
        &mut output,
    );

    for value in output {
        assert!((value - 10.0).abs() < 1.0e-6);
    }
}

#[test]
fn expression_supports_unary_minus_abs_and_ridge() {
    let program = compile_expression(
        "-x + abs(z) + ridge(0.25)",
        &BTreeMap::new(),
    )
        .expect("Failed to compile expression");

    let xs = [2.0, 4.0];
    let zs = [-4.0, -2.0];

    let mut output = [0.0; 2];

    execute_scalar(
        &program,
        &[],
        &xs,
        &zs,
        &mut output,
    );

    assert!((output[0] - 2.75).abs() < 1.0e-6);
    assert!((output[1] + 1.25).abs() < 1.0e-6);
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
fn scalar_execution_handles_unequal_buffer_lengths() {
    let program = compile_expression(
        "x + z",
        &BTreeMap::new(),
    )
        .unwrap();

    let xs = [1.0, 2.0, 3.0, 4.0];
    let zs = [10.0, 20.0, 30.0];

    let mut output = [999.0; 4];

    execute_scalar(
        &program,
        &[],
        &xs,
        &zs,
        &mut output,
    );

    assert_eq!(output, [11.0, 22.0, 33.0, 999.0]);
}

#[test]
fn continental_terrain_is_finite_and_non_flat() {
    let preset = continental_preset();

    let side = 65;
    let count = side * side;

    let mut xs = Vec::with_capacity(count);
    let mut zs = Vec::with_capacity(count);

    for z in 0..side {
        for x in 0..side {
            xs.push((x as f32 - 32.0) * 32.0);
            zs.push((z as f32 - 32.0) * 32.0);
        }
    }

    let mut output = vec![0.0; count];
    let mut workspace = GenWorkspace::new(&preset.program);

    workspace.execute(
        &preset.program,
        &preset.noises,
        &xs,
        &zs,
        &mut output,
    );

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
fn automatic_backend_matches_scalar() {
    let preset = continental_preset();
    let mut workspace = GenWorkspace::new(&preset.program);

    assert_eq!(workspace.backend(), GenBackend::detected());

    for count in [0, 1, 3, 4, 7, 8, 15, 16, 17, 31, 33, 65, 257] {
        let (xs, zs) = generate_coordinates(count);

        let mut expected = vec![0.0; count];
        let mut actual = vec![0.0; count];

        execute_scalar(
            &preset.program,
            &preset.noises,
            &xs,
            &zs,
            &mut expected,
        );

        workspace.execute(
            &preset.program,
            &preset.noises,
            &xs,
            &zs,
            &mut actual,
        );

        assert_close(&actual, &expected);
    }
}

#[cfg(target_arch = "x86_64")]
#[test]
fn explicit_avx2_backend_matches_scalar() {
    if !std::arch::is_x86_feature_detected!("avx2") {
        return;
    }

    let preset = continental_preset();
    let (xs, zs) = generate_coordinates(37);

    let mut expected = vec![0.0; xs.len()];
    let mut actual = vec![0.0; xs.len()];

    execute_scalar(
        &preset.program,
        &preset.noises,
        &xs,
        &zs,
        &mut expected,
    );

    assert!(execute_avx2(
        &preset.program,
        &preset.noises,
        &xs,
        &zs,
        &mut actual,
    ));

    assert_close(&actual, &expected);
}

#[test]
fn reusable_scalar_workspace_matches_scalar_executor() {
    let preset = continental_preset();
    let (xs, zs) = generate_coordinates(129);

    let mut expected = vec![0.0; xs.len()];
    let mut actual = vec![0.0; xs.len()];

    let mut workspace = ScalarWorkspace::new(&preset.program);

    execute_scalar(
        &preset.program,
        &preset.noises,
        &xs,
        &zs,
        &mut expected,
    );

    execute_scalar_with_workspace(
        &preset.program,
        &preset.noises,
        &xs,
        &zs,
        &mut actual,
        &mut workspace,
    );

    assert_close(&actual, &expected);
}

#[test]
#[ignore = "Manual performance benchmark. Run with cargo test --release -- --ignored --nocapture"]
fn benchmark_continental_generation() {
    let preset = continental_preset();

    let side = 256;
    let count = side * side;
    let iterations = 8;

    let (xs, zs) = generate_coordinates(count);

    let mut scalar_output = vec![0.0; count];
    let mut automatic_output = vec![0.0; count];

    let mut scalar_workspace = ScalarWorkspace::new(&preset.program);
    let mut automatic_workspace = GenWorkspace::new(&preset.program);

    execute_scalar_with_workspace(
        &preset.program,
        &preset.noises,
        &xs,
        &zs,
        &mut scalar_output,
        &mut scalar_workspace,
    );

    automatic_workspace.execute(
        &preset.program,
        &preset.noises,
        &xs,
        &zs,
        &mut automatic_output,
    );

    assert_close(&automatic_output, &scalar_output);

    let scalar_start = Instant::now();

    for _ in 0..iterations {
        execute_scalar_with_workspace(
            &preset.program,
            &preset.noises,
            &xs,
            &zs,
            &mut scalar_output,
            &mut scalar_workspace,
        );
    }

    let scalar_elapsed = scalar_start.elapsed();

    let automatic_start = Instant::now();

    for _ in 0..iterations {
        automatic_workspace.execute(
            &preset.program,
            &preset.noises,
            &xs,
            &zs,
            &mut automatic_output,
        );
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
    println!("Backend:             {:?}", automatic_workspace.backend());
    println!("Samples per pass:    {count}");
    println!("Iterations:          {iterations}");
    println!("Scalar time:         {scalar_elapsed:?}");
    println!("Automatic time:      {automatic_elapsed:?}");
    println!("Scalar throughput:   {scalar_rate:.2} M samples/s");
    println!("Automatic throughput:{automatic_rate:.2} M samples/s");
    println!("Speedup:             {speedup:.2}x");
    println!("==============================================");
}
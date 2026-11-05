use std::collections::BTreeMap;

use crate::compiler::{Program, compile_expression};
use serde::Deserialize;
#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;
use std::path::Path;

#[derive(Deserialize)]
pub struct PresetFile {
    name: String,
    noises: BTreeMap<String, NoiseDefinition>,
    outputs: BTreeMap<String, String>,
}
impl Default for PresetFile {
    fn default() -> Self {
        let mut noises = BTreeMap::new();

        noises.insert(
            "continents".to_owned(),
            NoiseDefinition {
                seed: 1741,
                frequency: 0.0050,
                octaves: 5,
                lacunarity: 2.0,
                gain: 0.50,
            },
        );

        noises.insert(
            "mountains".to_owned(),
            NoiseDefinition {
                seed: 8197,
                frequency: 0.0180,
                octaves: 4,
                lacunarity: 2.05,
                gain: 0.48,
            },
        );

        let mut outputs = BTreeMap::new();

        outputs.insert(
            "height".to_owned(),
            "fbm(continents, x, z) * 1.10 + (ridge(fbm(mountains, x, z)) - 0.72) * 0.48".to_owned(),
        );

        Self {
            name: "Default".to_owned(),
            noises,
            outputs
        }
    }
}
#[derive(Deserialize)]
pub struct NoiseDefinition {
    seed: i32,
    frequency: f32,
    octaves: usize,
    lacunarity: f32,
    gain: f32,
}

#[derive(Clone)]
pub struct CompiledPreset {
    pub name: String,
    pub noises: Vec<NoiseConfig>,
    pub noise_ids: BTreeMap<String, u16>,
    pub program: Program,
    pub expression: String,
}
impl Default for CompiledPreset {
    fn default() -> Self {
        let file = PresetFile::default();
        compile_preset_file_default(file)
    }
}

pub fn compile_preset(path: impl AsRef<Path>) -> Result<CompiledPreset, String> {
    let path = path.as_ref();
    let contents = match std::fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(err) => {
            return Err(format!(
                "Failed to read Terrain Preset file '{}'. Error: {}",
                path.display(),
                err
            ));
        }
    };

    let parsed = match serde_yaml::from_str::<PresetFile>(&contents) {
        Ok(sfx) => sfx,
        Err(err) => {
            return Err(format!(
                "Failed to deserialize Terrain Preset file '{}'. Expected a valid PresetFile YAML. Error: {}",
                path.display(),
                err
            ));
        }
    };
    compile_preset_file(parsed)
}
pub fn compile_preset_file(parsed: PresetFile) -> Result<CompiledPreset, String> {
    if parsed.noises.is_empty() {
        return Err("The preset must define at least one noise source".to_owned());
    }

    let mut noises = Vec::with_capacity(parsed.noises.len());
    let mut noise_names = BTreeMap::new();

    for (index, (name, definition)) in parsed.noises.iter().enumerate() {
        if !definition.frequency.is_finite() || definition.frequency <= 0.0 {
            return Err(format!("Noise '{name}' has an invalid frequency"));
        }
        if !(1..=12).contains(&definition.octaves) {
            return Err(format!("Noise '{name}' must use between 1 and 12 octaves"));
        }
        if !definition.lacunarity.is_finite() || definition.lacunarity <= 0.0 {
            return Err(format!("Noise '{name}' has an invalid lacunarity"));
        }
        if !definition.gain.is_finite() || !(0.0..=1.0).contains(&definition.gain) {
            return Err(format!("Noise '{name}' gain must be between 0 and 1"));
        }
        let id = u16::try_from(index).map_err(|_| "Too many noise sources".to_owned())?;
        noise_names.insert(name.clone(), id);
        noises.push(NoiseConfig {
            seed: definition.seed,
            frequency: definition.frequency,
            octaves: definition.octaves,
            lacunarity: definition.lacunarity,
            gain: definition.gain,
        });
    }

    let expression = parsed
        .outputs
        .get("height")
        .cloned()
        .ok_or_else(|| "The preset must define outputs.height".to_owned())?;
    let program = compile_expression(&expression, &noise_names)?;

    Ok(CompiledPreset {
        name: parsed.name,
        noises,
        noise_ids: noise_names,
        program,
        expression,
    })
}
pub fn compile_preset_file_default(parsed: PresetFile) -> CompiledPreset {
    let mut noises = Vec::with_capacity(parsed.noises.len());
    let mut noise_names = BTreeMap::new();

    for (index, (name, definition)) in parsed.noises.iter().enumerate() {
        let Ok(id) = u16::try_from(index) else {
            break;
        };

        let frequency = if definition.frequency.is_finite() && definition.frequency > 0.0 {
            definition.frequency
        } else {
            0.001
        };

        let octaves = definition.octaves.clamp(1, 12);

        let lacunarity = if definition.lacunarity.is_finite() && definition.lacunarity > 0.0 {
            definition.lacunarity
        } else {
            2.0
        };

        let gain = if definition.gain.is_finite() && (0.0..=1.0).contains(&definition.gain) {
            definition.gain
        } else {
            0.5
        };

        noise_names.insert(name.clone(), id);

        noises.push(NoiseConfig {
            seed: definition.seed,
            frequency,
            octaves,
            lacunarity,
            gain,
        });
    }

    let expression = parsed
        .outputs
        .get("height")
        .cloned()
        .unwrap_or_else(|| "0.0".to_owned());

    let (expression, program) = match compile_expression(&expression, &noise_names) {
        Ok(program) => (expression, program),
        Err(_) => {
            let expression = "0.0".to_owned();
            let program = compile_expression(&expression, &noise_names)
                .expect("The constant fallback expression must compile");

            (expression, program)
        }
    };

    CompiledPreset {
        name: parsed.name,
        noises,
        noise_ids: noise_names,
        program,
        expression,
    }
}
#[derive(Clone, Debug)]
pub struct NoiseConfig {
    pub seed: i32,
    pub frequency: f32,
    pub octaves: usize,
    pub lacunarity: f32,
    pub gain: f32,
}

const GRADIENT_X: [f32; 8] = [1.0, -1.0, 0.0, 0.0, std::f32::consts::FRAC_1_SQRT_2, -std::f32::consts::FRAC_1_SQRT_2, std::f32::consts::FRAC_1_SQRT_2, -std::f32::consts::FRAC_1_SQRT_2];
const GRADIENT_Z: [f32; 8] = [0.0, 0.0, 1.0, -1.0, std::f32::consts::FRAC_1_SQRT_2, std::f32::consts::FRAC_1_SQRT_2, -std::f32::consts::FRAC_1_SQRT_2, -std::f32::consts::FRAC_1_SQRT_2];
const NOISE_SCALE: f32 = std::f32::consts::SQRT_2;

#[inline]
fn hash2(x: i32, z: i32, seed: i32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343)
        ^ (z as u32).wrapping_mul(0xd816_3841)
        ^ seed as u32;
    h ^= h >> 13;
    h = h.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 16;
    h
}

#[inline]
fn fade(t: f32) -> f32 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

#[inline]
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + t * (b - a)
}

#[inline]
fn gradient_dot(ix: i32, iz: i32, dx: f32, dz: f32, seed: i32) -> f32 {
    let index = (hash2(ix, iz, seed) & 7) as usize;
    GRADIENT_X[index] * dx + GRADIENT_Z[index] * dz
}

#[inline]
pub fn noise2d(x: f32, z: f32, seed: i32) -> f32 {
    let floor_x = x.floor();
    let floor_z = z.floor();
    let ix = floor_x as i32;
    let iz = floor_z as i32;
    let fx = x - floor_x;
    let fz = z - floor_z;

    let n00 = gradient_dot(ix, iz, fx, fz, seed);
    let n10 = gradient_dot(ix.wrapping_add(1), iz, fx - 1.0, fz, seed);
    let n01 = gradient_dot(ix, iz.wrapping_add(1), fx, fz - 1.0, seed);
    let n11 = gradient_dot(ix.wrapping_add(1), iz.wrapping_add(1), fx - 1.0, fz - 1.0, seed);

    let u = fade(fx);
    let v = fade(fz);
    lerp(lerp(n00, n10, u), lerp(n01, n11, u), v) * NOISE_SCALE
}

#[inline]
pub fn fbm_scalar(config: &NoiseConfig, x: f32, z: f32) -> f32 {
    let mut frequency = config.frequency;
    let mut amplitude = 1.0_f32;
    let mut amplitude_sum = 0.0_f32;
    let mut value = 0.0_f32;

    for octave in 0..config.octaves {
        let seed = config.seed.wrapping_add((octave as i32).wrapping_mul(1013));
        value += noise2d(x * frequency, z * frequency, seed) * amplitude;
        amplitude_sum += amplitude;
        frequency *= config.lacunarity;
        amplitude *= config.gain;
    }

    if amplitude_sum > 0.0 {
        value / amplitude_sum
    } else {
        0.0
    }
}


#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn hash2_avx2(x: __m256i, z: __m256i, seed: i32) -> __m256i {
    let mut h = _mm256_xor_si256(
        _mm256_mullo_epi32(x, _mm256_set1_epi32(0x8da6_b343_u32 as i32)),
        _mm256_mullo_epi32(z, _mm256_set1_epi32(0xd816_3841_u32 as i32)),
    );
    h = _mm256_xor_si256(h, _mm256_set1_epi32(seed));
    h = _mm256_xor_si256(h, _mm256_srli_epi32::<13>(h));
    h = _mm256_mullo_epi32(h, _mm256_set1_epi32(0x85eb_ca6b_u32 as i32));
    _mm256_xor_si256(h, _mm256_srli_epi32::<16>(h))
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn gradient_dot_avx2(
    ix: __m256i,
    iz: __m256i,
    dx: __m256,
    dz: __m256,
    seed: i32,
) -> __m256 {
    let indices = _mm256_and_si256(unsafe {hash2_avx2(ix, iz, seed)}, _mm256_set1_epi32(7));
    let gx = unsafe {_mm256_i32gather_ps(GRADIENT_X.as_ptr(), indices, 4)};
    let gz = unsafe {_mm256_i32gather_ps(GRADIENT_Z.as_ptr(), indices, 4)};
    _mm256_add_ps(_mm256_mul_ps(gx, dx), _mm256_mul_ps(gz, dz))
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn fade_avx2(t: __m256) -> __m256 {
    let six = _mm256_set1_ps(6.0);
    let fifteen = _mm256_set1_ps(15.0);
    let ten = _mm256_set1_ps(10.0);
    let t2 = _mm256_mul_ps(t, t);
    let t3 = _mm256_mul_ps(t2, t);
    let inner = _mm256_add_ps(
        _mm256_mul_ps(t, _mm256_sub_ps(_mm256_mul_ps(t, six), fifteen)),
        ten,
    );
    _mm256_mul_ps(t3, inner)
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn lerp_avx2(a: __m256, b: __m256, t: __m256) -> __m256 {
    _mm256_add_ps(a, _mm256_mul_ps(t, _mm256_sub_ps(b, a)))
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
unsafe fn lerp_fma(a: __m256, b: __m256, t: __m256) -> __m256 {
    _mm256_fmadd_ps(t, _mm256_sub_ps(b, a), a)
}
#[cfg(target_arch = "x86_64")]
unsafe fn lerp_ext(a: __m256, b: __m256, t: __m256) -> __m256 { unsafe {
    if is_x86_feature_detected!("avx2") && is_x86_feature_detected!("fma") {
        lerp_fma(a, b, t)
    } else if is_x86_feature_detected!("avx2") {
        lerp_avx2(a, b, t)
    } else {
        // Scalar fallback or another implementation.
        unreachable!("AVX2 implementation called without AVX2 support")
    }}
}


#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
pub unsafe fn noise2d_avx2(x: __m256, z: __m256, seed: i32) -> __m256 { unsafe {
    let floor_x = _mm256_floor_ps(x);
    let floor_z = _mm256_floor_ps(z);
    let ix = _mm256_cvttps_epi32(floor_x);
    let iz = _mm256_cvttps_epi32(floor_z);
    let fx = _mm256_sub_ps(x, floor_x);
    let fz = _mm256_sub_ps(z, floor_z);
    let one = _mm256_set1_ps(1.0);

    let n00 = gradient_dot_avx2(ix, iz, fx, fz, seed);
    let n10 = gradient_dot_avx2(
        _mm256_add_epi32(ix, _mm256_set1_epi32(1)),
        iz,
        _mm256_sub_ps(fx, one),
        fz,
        seed,
    );
    let n01 = gradient_dot_avx2(
        ix,
        _mm256_add_epi32(iz, _mm256_set1_epi32(1)),
        fx,
        _mm256_sub_ps(fz, one),
        seed,
    );
    let n11 = gradient_dot_avx2(
        _mm256_add_epi32(ix, _mm256_set1_epi32(1)),
        _mm256_add_epi32(iz, _mm256_set1_epi32(1)),
        _mm256_sub_ps(fx, one),
        _mm256_sub_ps(fz, one),
        seed,
    );

    let u = fade_avx2(fx);
    let v = fade_avx2(fz);
    let lower = lerp_avx2(n00, n10, u);
    let upper = lerp_avx2(n01, n11, u);
    _mm256_mul_ps(lerp_ext(lower, upper, v), _mm256_set1_ps(NOISE_SCALE))
}}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
pub unsafe fn fbm_avx2(config: &NoiseConfig, x: __m256, z: __m256) -> __m256 { unsafe {
    let mut frequency = config.frequency;
    let mut amplitude = 1.0_f32;
    let mut amplitude_sum = 0.0_f32;
    let mut value = _mm256_setzero_ps();

    for octave in 0..config.octaves {
        let seed = config.seed.wrapping_add((octave as i32).wrapping_mul(1013));
        let sx = _mm256_mul_ps(x, _mm256_set1_ps(frequency));
        let sz = _mm256_mul_ps(z, _mm256_set1_ps(frequency));
        let octave_value = noise2d_avx2(sx, sz, seed);
        value = _mm256_add_ps(
            value,
            _mm256_mul_ps(octave_value, _mm256_set1_ps(amplitude)),
        );
        amplitude_sum += amplitude;
        frequency *= config.lacunarity;
        amplitude *= config.gain;
    }

    if amplitude_sum > 0.0 {
        _mm256_div_ps(value, _mm256_set1_ps(amplitude_sum))
    } else {
        _mm256_setzero_ps()
    }

}}

pub fn avx2_available() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        is_x86_feature_detected!("avx2")
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        false
    }
}


#[inline(always)]
fn noise2d_fma_scalar(x: f32, z: f32, seed: i32) -> f32 {
    let floor_x = x.floor();
    let floor_z = z.floor();
    let ix = floor_x as i32;
    let iz = floor_z as i32;
    let fx = x - floor_x;
    let fz = z - floor_z;

    let dot = |ix: i32, iz: i32, dx: f32, dz: f32| {
        let index = (hash2(ix, iz, seed) & 7) as usize;
        GRADIENT_X[index].mul_add(dx, GRADIENT_Z[index] * dz)
    };

    let n00 = dot(ix, iz, fx, fz);
    let n10 = dot(ix.wrapping_add(1), iz, fx - 1.0, fz);
    let n01 = dot(ix, iz.wrapping_add(1), fx, fz - 1.0);
    let n11 = dot(ix.wrapping_add(1), iz.wrapping_add(1), fx - 1.0, fz - 1.0);

    let fade_fma = |t: f32| {
        let inner = t.mul_add(t.mul_add(6.0, -15.0), 10.0);
        (t * t * t) * inner
    };

    let lerp_fma = |a: f32, b: f32, t: f32| {
        (b - a).mul_add(t, a)
    };

    let u = fade_fma(fx);
    let v = fade_fma(fz);

    lerp_fma(
        lerp_fma(n00, n10, u),
        lerp_fma(n01, n11, u),
        v,
    ) * NOISE_SCALE
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "fma")]
pub unsafe fn fbm_scalar_fma(
    config: &NoiseConfig,
    x: f32,
    z: f32,
) -> f32 {
    let mut frequency = config.frequency;
    let mut amplitude = 1.0_f32;
    let mut amplitude_sum = 0.0_f32;
    let mut value = 0.0_f32;

    for octave in 0..config.octaves {
        let seed = config.seed.wrapping_add((octave as i32).wrapping_mul(1013));
        value = noise2d_fma_scalar(x * frequency, z * frequency, seed)
            .mul_add(amplitude, value);

        amplitude_sum += amplitude;
        frequency *= config.lacunarity;
        amplitude *= config.gain;
    }

    if amplitude_sum > 0.0 {
        value / amplitude_sum
    } else {
        0.0
    }
}
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
unsafe fn gradient_dot_avx2_fma(
    ix: __m256i,
    iz: __m256i,
    dx: __m256,
    dz: __m256,
    seed: i32,
) -> __m256 {
    unsafe {
        let indices = _mm256_and_si256(
            hash2_avx2(ix, iz, seed),
            _mm256_set1_epi32(7),
        );

        let gx = _mm256_i32gather_ps(GRADIENT_X.as_ptr(), indices, 4);
        let gz = _mm256_i32gather_ps(GRADIENT_Z.as_ptr(), indices, 4);

        _mm256_fmadd_ps(gx, dx, _mm256_mul_ps(gz, dz))
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
unsafe fn fade_avx2_fma(t: __m256) -> __m256 {
    let six = _mm256_set1_ps(6.0);
    let fifteen = _mm256_set1_ps(-15.0);
    let ten = _mm256_set1_ps(10.0);

    let t2 = _mm256_mul_ps(t, t);
    let t3 = _mm256_mul_ps(t2, t);

    let inner = _mm256_fmadd_ps(
        t,
        _mm256_fmadd_ps(t, six, fifteen),
        ten,
    );

    _mm256_mul_ps(t3, inner)
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
unsafe fn lerp_avx2_fma(a: __m256, b: __m256, t: __m256) -> __m256 {
    _mm256_fmadd_ps(t, _mm256_sub_ps(b, a), a)
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
pub unsafe fn noise2d_avx2_fma(
    x: __m256,
    z: __m256,
    seed: i32,
) -> __m256 {
    unsafe {
        let floor_x = _mm256_floor_ps(x);
        let floor_z = _mm256_floor_ps(z);

        let ix = _mm256_cvttps_epi32(floor_x);
        let iz = _mm256_cvttps_epi32(floor_z);

        let fx = _mm256_sub_ps(x, floor_x);
        let fz = _mm256_sub_ps(z, floor_z);
        let one = _mm256_set1_ps(1.0);

        let n00 = gradient_dot_avx2_fma(ix, iz, fx, fz, seed);

        let n10 = gradient_dot_avx2_fma(
            _mm256_add_epi32(ix, _mm256_set1_epi32(1)),
            iz,
            _mm256_sub_ps(fx, one),
            fz,
            seed,
        );

        let n01 = gradient_dot_avx2_fma(
            ix,
            _mm256_add_epi32(iz, _mm256_set1_epi32(1)),
            fx,
            _mm256_sub_ps(fz, one),
            seed,
        );

        let n11 = gradient_dot_avx2_fma(
            _mm256_add_epi32(ix, _mm256_set1_epi32(1)),
            _mm256_add_epi32(iz, _mm256_set1_epi32(1)),
            _mm256_sub_ps(fx, one),
            _mm256_sub_ps(fz, one),
            seed,
        );

        let u = fade_avx2_fma(fx);
        let v = fade_avx2_fma(fz);

        let lower = lerp_avx2_fma(n00, n10, u);
        let upper = lerp_avx2_fma(n01, n11, u);

        _mm256_mul_ps(
            lerp_avx2_fma(lower, upper, v),
            _mm256_set1_ps(NOISE_SCALE),
        )
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
pub unsafe fn fbm_avx2_fma(
    config: &NoiseConfig,
    x: __m256,
    z: __m256,
) -> __m256 {
    unsafe {
        let mut frequency = config.frequency;
        let mut amplitude = 1.0_f32;
        let mut amplitude_sum = 0.0_f32;
        let mut value = _mm256_setzero_ps();

        for octave in 0..config.octaves {
            let seed = config.seed.wrapping_add((octave as i32).wrapping_mul(1013));

            let sx = _mm256_mul_ps(x, _mm256_set1_ps(frequency));
            let sz = _mm256_mul_ps(z, _mm256_set1_ps(frequency));

            let octave_value = noise2d_avx2_fma(sx, sz, seed);

            value = _mm256_fmadd_ps(
                octave_value,
                _mm256_set1_ps(amplitude),
                value,
            );

            amplitude_sum += amplitude;
            frequency *= config.lacunarity;
            amplitude *= config.gain;
        }

        if amplitude_sum > 0.0 {
            _mm256_div_ps(value, _mm256_set1_ps(amplitude_sum))
        } else {
            _mm256_setzero_ps()
        }
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f")]
unsafe fn hash2_avx512(
    x: __m512i,
    z: __m512i,
    seed: i32,
) -> __m512i {
    let mut h = _mm512_xor_si512(
        _mm512_mullo_epi32(
            x,
            _mm512_set1_epi32(0x8da6_b343_u32 as i32),
        ),
        _mm512_mullo_epi32(
            z,
            _mm512_set1_epi32(0xd816_3841_u32 as i32),
        ),
    );

    h = _mm512_xor_si512(h, _mm512_set1_epi32(seed));
    h = _mm512_xor_si512(h, _mm512_srli_epi32::<13>(h));

    h = _mm512_mullo_epi32(
        h,
        _mm512_set1_epi32(0x85eb_ca6b_u32 as i32),
    );

    _mm512_xor_si512(h, _mm512_srli_epi32::<16>(h))

}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f")]
unsafe fn gradient_dot_avx512(
    ix: __m512i,
    iz: __m512i,
    dx: __m512,
    dz: __m512,
    seed: i32,
) -> __m512 {
    unsafe {
        let indices = _mm512_and_si512(
            hash2_avx512(ix, iz, seed),
            _mm512_set1_epi32(7),
        );

        let gx = _mm512_i32gather_ps(indices, GRADIENT_X.as_ptr(), 4);
        let gz = _mm512_i32gather_ps(indices, GRADIENT_Z.as_ptr(), 4);

        _mm512_fmadd_ps(gx, dx, _mm512_mul_ps(gz, dz))
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f")]
unsafe fn fade_avx512(t: __m512) -> __m512 {
    let six = _mm512_set1_ps(6.0);
    let fifteen = _mm512_set1_ps(-15.0);
    let ten = _mm512_set1_ps(10.0);

    let t2 = _mm512_mul_ps(t, t);
    let t3 = _mm512_mul_ps(t2, t);

    let inner = _mm512_fmadd_ps(
        t,
        _mm512_fmadd_ps(t, six, fifteen),
        ten,
    );

    _mm512_mul_ps(t3, inner)
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f")]
unsafe fn lerp_avx512(a: __m512, b: __m512, t: __m512) -> __m512 {
    _mm512_fmadd_ps(t, _mm512_sub_ps(b, a), a)
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f")]
pub unsafe fn noise2d_avx512(
    x: __m512,
    z: __m512,
    seed: i32,
) -> __m512 {
    unsafe {
        let floor_x = _mm512_roundscale_ps::<
            { _MM_FROUND_TO_NEG_INF | _MM_FROUND_NO_EXC }
        >(x);

        let floor_z = _mm512_roundscale_ps::<
            { _MM_FROUND_TO_NEG_INF | _MM_FROUND_NO_EXC }
        >(z);

        let ix = _mm512_cvttps_epi32(floor_x);
        let iz = _mm512_cvttps_epi32(floor_z);

        let fx = _mm512_sub_ps(x, floor_x);
        let fz = _mm512_sub_ps(z, floor_z);
        let one = _mm512_set1_ps(1.0);

        let n00 = gradient_dot_avx512(ix, iz, fx, fz, seed);

        let n10 = gradient_dot_avx512(
            _mm512_add_epi32(ix, _mm512_set1_epi32(1)),
            iz,
            _mm512_sub_ps(fx, one),
            fz,
            seed,
        );

        let n01 = gradient_dot_avx512(
            ix,
            _mm512_add_epi32(iz, _mm512_set1_epi32(1)),
            fx,
            _mm512_sub_ps(fz, one),
            seed,
        );

        let n11 = gradient_dot_avx512(
            _mm512_add_epi32(ix, _mm512_set1_epi32(1)),
            _mm512_add_epi32(iz, _mm512_set1_epi32(1)),
            _mm512_sub_ps(fx, one),
            _mm512_sub_ps(fz, one),
            seed,
        );

        let u = fade_avx512(fx);
        let v = fade_avx512(fz);

        let lower = lerp_avx512(n00, n10, u);
        let upper = lerp_avx512(n01, n11, u);

        _mm512_mul_ps(
            lerp_avx512(lower, upper, v),
            _mm512_set1_ps(NOISE_SCALE),
        )
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f")]
pub unsafe fn fbm_avx512_fma(
    config: &NoiseConfig,
    x: __m512,
    z: __m512,
) -> __m512 {
    unsafe {
        let mut frequency = config.frequency;
        let mut amplitude = 1.0_f32;
        let mut amplitude_sum = 0.0_f32;
        let mut value = _mm512_setzero_ps();

        for octave in 0..config.octaves {
            let seed = config.seed.wrapping_add((octave as i32).wrapping_mul(1013));

            let sx = _mm512_mul_ps(x, _mm512_set1_ps(frequency));
            let sz = _mm512_mul_ps(z, _mm512_set1_ps(frequency));

            let octave_value = noise2d_avx512(sx, sz, seed);

            value = _mm512_fmadd_ps(
                octave_value,
                _mm512_set1_ps(amplitude),
                value,
            );

            amplitude_sum += amplitude;
            frequency *= config.lacunarity;
            amplitude *= config.gain;
        }

        if amplitude_sum > 0.0 {
            _mm512_div_ps(value, _mm512_set1_ps(amplitude_sum))
        } else {
            _mm512_setzero_ps()
        }
    }
}



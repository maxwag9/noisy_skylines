
use std::collections::BTreeMap;
use crate::presets::{fbm_scalar, NoiseConfig, fbm_f64, fbm_dvec2_f32, CompiledPreset};

#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;
use glam::{DVec2, Vec2};
use strum_macros::Display;
#[cfg(target_arch = "x86_64")]
use crate::presets::{
    fbm_avx2,
    fbm_avx2_fma,
    fbm_avx512_fma,
    fbm_scalar_fma,
};

#[derive(Clone, Copy, Debug)]
enum Token {
    Number(f64),
    Ident(usize, usize),
    Plus,
    Minus,
    Star,
    Slash,
    LeftParen,
    RightParen,
    Comma,
    End,
}
struct Lexer<'a> {
    source: &'a str,
    cursor: usize,
}

impl<'a> Lexer<'a> {
    fn new(source: &'a str) -> Self {
        Self { source, cursor: 0 }
    }

    fn tokenize(mut self) -> Result<Vec<Token>, String> {
        let bytes = self.source.as_bytes();
        let mut tokens = Vec::new();

        while self.cursor < bytes.len() {
            let byte = bytes[self.cursor];
            if byte.is_ascii_whitespace() {
                self.cursor += 1;
                continue;
            }

            let token = match byte {
                b'+' => {
                    self.cursor += 1;
                    Token::Plus
                }
                b'-' => {
                    self.cursor += 1;
                    Token::Minus
                }
                b'*' => {
                    self.cursor += 1;
                    Token::Star
                }
                b'/' => {
                    self.cursor += 1;
                    Token::Slash
                }
                b'(' => {
                    self.cursor += 1;
                    Token::LeftParen
                }
                b')' => {
                    self.cursor += 1;
                    Token::RightParen
                }
                b',' => {
                    self.cursor += 1;
                    Token::Comma
                }
                b'0'..=b'9' | b'.' => {
                    let start = self.cursor;
                    while self.cursor < bytes.len()
                        && (bytes[self.cursor].is_ascii_digit() || bytes[self.cursor] == b'.')
                    {
                        self.cursor += 1;
                    }
                    if self.cursor < bytes.len()
                        && (bytes[self.cursor] == b'e' || bytes[self.cursor] == b'E')
                    {
                        self.cursor += 1;
                        if self.cursor < bytes.len()
                            && (bytes[self.cursor] == b'+' || bytes[self.cursor] == b'-')
                        {
                            self.cursor += 1;
                        }
                        while self.cursor < bytes.len() && bytes[self.cursor].is_ascii_digit() {
                            self.cursor += 1;
                        }
                    }
                    let value = self.source[start..self.cursor]
                        .parse::<f64>()
                        .map_err(|_| format!("Invalid number at byte {start}"))?;
                    if !value.is_finite() {
                        return Err(format!("Non-finite number at byte {start}"));
                    }
                    Token::Number(value)
                }
                b'a'..=b'z' | b'A'..=b'Z' | b'_' => {
                    let start = self.cursor;
                    self.cursor += 1;
                    while self.cursor < bytes.len()
                        && (bytes[self.cursor].is_ascii_alphanumeric()
                        || bytes[self.cursor] == b'_')
                    {
                        self.cursor += 1;
                    }
                    Token::Ident(start, self.cursor)
                }
                _ => return Err(format!("Unexpected character '{}' at byte {}", byte as char, self.cursor)),
            };
            tokens.push(token);
        }

        tokens.push(Token::End);
        Ok(tokens)
    }
}

#[derive(Debug)]
enum Expr {
    Number(f64),
    Name(String),
    UnaryMinus(Box<Expr>),
    Binary {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Call {
        name: String,
        args: Vec<Expr>,
    },
}

#[derive(Clone, Copy, Debug)]
enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
}

struct Parser<'a> {
    source: &'a str,
    tokens: Vec<Token>,
    cursor: usize,
}

impl<'a> Parser<'a> {
    fn new(source: &'a str) -> Result<Self, String> {
        Ok(Self {
            source,
            tokens: Lexer::new(source).tokenize()?,
            cursor: 0,
        })
    }

    fn parse(mut self) -> Result<Expr, String> {
        let expression = self.parse_binding_power(0)?;
        if !matches!(self.peek(), Token::End) {
            return Err("Unexpected token after expression".to_owned());
        }
        Ok(expression)
    }

    fn peek(&self) -> Token {
        self.tokens[self.cursor]
    }

    fn next(&mut self) -> Token {
        let token = self.tokens[self.cursor];
        self.cursor += 1;
        token
    }

    fn ident_text(&self, start: usize, end: usize) -> String {
        self.source[start..end].to_owned()
    }

    fn parse_binding_power(&mut self, min_binding_power: u8) -> Result<Expr, String> {
        let mut left = match self.next() {
            Token::Number(value) => Expr::Number(value),
            Token::Ident(start, end) => {
                let name = self.ident_text(start, end);
                if matches!(self.peek(), Token::LeftParen) {
                    self.next();
                    let mut args = Vec::new();
                    if !matches!(self.peek(), Token::RightParen) {
                        loop {
                            args.push(self.parse_binding_power(0)?);
                            if matches!(self.peek(), Token::Comma) {
                                self.next();
                                continue;
                            }
                            break;
                        }
                    }
                    if !matches!(self.next(), Token::RightParen) {
                        return Err(format!("Expected ')' after call to {name}"));
                    }
                    Expr::Call { name, args }
                } else {
                    Expr::Name(name)
                }
            }
            Token::Minus => Expr::UnaryMinus(Box::new(self.parse_binding_power(5)?)),
            Token::LeftParen => {
                let expression = self.parse_binding_power(0)?;
                if !matches!(self.next(), Token::RightParen) {
                    return Err("Expected ')'".to_owned());
                }
                expression
            }
            _ => return Err("Expected a number, variable, function call or '('".to_owned()),
        };

        loop {
            let (left_bp, right_bp, op) = match self.peek() {
                Token::Plus => (1, 2, BinaryOp::Add),
                Token::Minus => (1, 2, BinaryOp::Sub),
                Token::Star => (3, 4, BinaryOp::Mul),
                Token::Slash => (3, 4, BinaryOp::Div),
                _ => break,
            };

            if left_bp < min_binding_power {
                break;
            }
            self.next();
            let right = self.parse_binding_power(right_bp)?;
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }

        Ok(left)
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Axis {
    X,
    Z,
}

#[derive(Clone, Copy, Debug)]
pub enum Instruction {
    Input { dst: u16, axis: Axis },
    Constant { dst: u16, value_f32: f32, value_f64: f64 },
    Add { dst: u16, a: u16, b: u16 },
    Sub { dst: u16, a: u16, b: u16 },
    Mul { dst: u16, a: u16, b: u16 },
    Div { dst: u16, a: u16, b: u16 },
    Fbm { dst: u16, noise: u16 },
    Ridge { dst: u16, source: u16 },
    Abs { dst: u16, source: u16 }
}

#[derive(Clone, Debug)]
pub struct Program {
    pub instructions: Vec<Instruction>,
    pub register_count: usize,
    pub output: u16,
}

struct Compiler<'a> {
    noises: &'a BTreeMap<String, u16>,
    instructions: Vec<Instruction>,
    next_register: u16,
}

impl<'a> Compiler<'a> {
    fn new(noises: &'a BTreeMap<String, u16>) -> Self {
        Self {
            noises,
            instructions: vec![
                Instruction::Input { dst: 0, axis: Axis::X },
                Instruction::Input { dst: 1, axis: Axis::Z },
            ],
            next_register: 2,
        }
    }

    fn allocate(&mut self) -> Result<u16, String> {
        let register = self.next_register;
        self.next_register = self
            .next_register
            .checked_add(1)
            .ok_or_else(|| "Expression requires too many registers".to_owned())?;
        Ok(register)
    }

    fn compile_expr(&mut self, expression: &Expr) -> Result<u16, String> {
        match expression {
            Expr::Number(value) => {
                let dst = self.allocate()?;

                self.instructions.push(Instruction::Constant {
                    dst,
                    value_f32: *value as f32,
                    value_f64: *value,
                });

                Ok(dst)
            }
            Expr::Name(name) => match name.as_str() {
                "x" => Ok(0),
                "z" => Ok(1),
                _ => Err(format!("Unknown variable '{name}'. Only x and z are coordinate variables.")),
            },
            Expr::UnaryMinus(value) => {
                let source = self.compile_expr(value)?;
                let zero = self.allocate()?;
                self.instructions.push(Instruction::Constant { dst: zero, value_f32: 0.0, value_f64: 0.0 });
                let dst = self.allocate()?;
                self.instructions.push(Instruction::Sub { dst, a: zero, b: source });
                Ok(dst)
            }
            Expr::Binary { op, left, right } => {
                let a = self.compile_expr(left)?;
                let b = self.compile_expr(right)?;
                let dst = self.allocate()?;
                let instruction = match op {
                    BinaryOp::Add => Instruction::Add { dst, a, b },
                    BinaryOp::Sub => Instruction::Sub { dst, a, b },
                    BinaryOp::Mul => Instruction::Mul { dst, a, b },
                    BinaryOp::Div => Instruction::Div { dst, a, b },
                };
                self.instructions.push(instruction);
                Ok(dst)
            }
            Expr::Call { name, args } => match name.as_str() {
                "fbm" => {
                    if args.len() != 1 {
                        return Err(
                            "fbm expects only fbm(noise_name)".to_owned(),
                        );
                    }

                    let noise_name = match &args[0] {
                        Expr::Name(name) => name,
                        _ => {
                            return Err(
                                "The first fbm argument must be a declared noise name"
                                    .to_owned(),
                            );
                        }
                    };

                    let noise = self
                        .noises
                        .get(noise_name)
                        .copied()
                        .ok_or_else(|| {
                            format!(
                                "Noise source '{noise_name}' was not declared in YAML"
                            )
                        })?;

                    let dst = self.allocate()?;

                    self.instructions.push(Instruction::Fbm {
                        dst,
                        noise,
                    });

                    Ok(dst)
                }
                "ridge" | "abs" => {
                    if args.len() != 1 {
                        return Err(format!("{name} expects exactly one argument"));
                    }
                    let source = self.compile_expr(&args[0])?;
                    let dst = self.allocate()?;
                    let instruction = if name == "ridge" {
                        Instruction::Ridge { dst, source }
                    } else {
                        Instruction::Abs { dst, source }
                    };
                    self.instructions.push(instruction);
                    Ok(dst)
                }
                _ => Err(format!("Unknown function '{name}'")),
            },
        }
    }
}

pub fn compile_expression(source: &str, noise_names: &BTreeMap<String, u16>) -> Result<Program, String> {
    let ast = Parser::new(source)?.parse()?;
    let mut compiler = Compiler::new(noise_names);
    let output = compiler.compile_expr(&ast)?;
    Ok(Program {
        instructions: compiler.instructions,
        register_count: compiler.next_register as usize,
        output,
    })
}


trait IntoDVec2 {
    fn into_dvec2(self) -> DVec2;
}

impl IntoDVec2 for Vec2 {
    #[inline]
    fn into_dvec2(self) -> DVec2 {
        DVec2::new(self.x as f64, self.y as f64)
    }
}

impl IntoDVec2 for DVec2 {
    #[inline]
    fn into_dvec2(self) -> DVec2 {
        self
    }
}

// #[derive(Clone, Copy, Debug, PartialEq, Eq)]
// pub enum GenBackend {
//     Avx512F,
//     Avx2Fma,
//     Avx2,
//     AvxFma,
//     Avx,
//     Sse2,
//     Scalar,
// }
// impl GenBackend {
//     pub fn detected() -> GenBackend {
//         #[cfg(target_arch = "x86_64")]
//         {
//             if is_x86_feature_detected!("avx512f") {
//                 return GenBackend::Avx512F;
//             }
//
//             if is_x86_feature_detected!("avx2")
//                 && is_x86_feature_detected!("fma")
//             {
//                 return GenBackend::Avx2Fma;
//             }
//
//             if is_x86_feature_detected!("avx2") {
//                 return GenBackend::Avx2;
//             }
//
//             if is_x86_feature_detected!("avx")
//                 && is_x86_feature_detected!("fma")
//             {
//                 return GenBackend::AvxFma;
//             }
//
//             if is_x86_feature_detected!("avx") {
//                 return GenBackend::Avx;
//             }
//
//             if is_x86_feature_detected!("sse2") {
//                 return GenBackend::Sse2;
//             }
//         }
//
//         GenBackend::Scalar
//     }
// }

// pub fn backend(&self) -> GenBackend {
//     match self {
//         Self::Scalar(_) => GenBackend::Scalar,
//
//         #[cfg(target_arch = "x86_64")]
//         Self::Sse2(_) => GenBackend::Sse2,
//
//         #[cfg(target_arch = "x86_64")]
//         Self::Avx(_) => GenBackend::Avx,
//
//         #[cfg(target_arch = "x86_64")]
//         Self::AvxFma(_) => GenBackend::AvxFma,
//
//         #[cfg(target_arch = "x86_64")]
//         Self::Avx2(_) => GenBackend::Avx2,
//
//         #[cfg(target_arch = "x86_64")]
//         Self::Avx2Fma(_) => GenBackend::Avx2Fma,
//
//         #[cfg(target_arch = "x86_64")]
//         Self::Avx512(_) => GenBackend::Avx512F,
//     }
// }



mod sealed {
    pub trait Point {}
    pub trait Output<P> {}
}

pub trait GenPoint: sealed::Point + Copy {
    const PRECISE_F32_FBM: bool;

    fn coordinates_f32(self) -> (f32, f32);
    fn coordinates_f64(self) -> (f64, f64);
    fn into_dvec2(self) -> DVec2;
    fn fbm_f32(self, config: &NoiseConfig) -> Option<f32>;
}

impl sealed::Point for Vec2 {}

impl GenPoint for Vec2 {
    const PRECISE_F32_FBM: bool = false;

    #[inline]
    fn coordinates_f32(self) -> (f32, f32) {
        (self.x, self.y)
    }

    #[inline]
    fn coordinates_f64(self) -> (f64, f64) {
        (self.x as f64, self.y as f64)
    }

    #[inline]
    fn into_dvec2(self) -> DVec2 {
        DVec2::new(self.x as f64, self.y as f64)
    }

    #[inline]
    fn fbm_f32(self, config: &NoiseConfig) -> Option<f32> {
        Some(fbm_scalar(config, self))
    }
}

impl sealed::Point for DVec2 {}

impl GenPoint for DVec2 {
    const PRECISE_F32_FBM: bool = true;

    #[inline]
    fn coordinates_f32(self) -> (f32, f32) {
        (self.x as f32, self.y as f32)
    }

    #[inline]
    fn coordinates_f64(self) -> (f64, f64) {
        (self.x, self.y)
    }

    #[inline]
    fn into_dvec2(self) -> DVec2 {
        self
    }

    #[inline]
    fn fbm_f32(self, config: &NoiseConfig) -> Option<f32> {
        fbm_dvec2_f32(config, self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Display)]
pub enum GenBackend {
    Avx512F,
    Avx2Fma,
    Avx2,
    AvxFma,
    Avx,
    Sse2,
    Scalar,
}

impl GenBackend {
    pub fn is_supported(self) -> bool {
        #[cfg(target_arch = "x86_64")]
        {
            match self {
                Self::Avx512F => {
                    std::arch::is_x86_feature_detected!("avx512f")
                }
                Self::Avx2Fma => {
                    std::arch::is_x86_feature_detected!("avx2")
                        && std::arch::is_x86_feature_detected!("fma")
                }
                Self::Avx2 => {
                    std::arch::is_x86_feature_detected!("avx2")
                }
                Self::AvxFma => {
                    std::arch::is_x86_feature_detected!("avx")
                        && std::arch::is_x86_feature_detected!("fma")
                }
                Self::Avx => {
                    std::arch::is_x86_feature_detected!("avx")
                }
                Self::Sse2 => {
                    std::arch::is_x86_feature_detected!("sse2")
                }
                Self::Scalar => true,
            }
        }

        #[cfg(not(target_arch = "x86_64"))]
        {
            matches!(self, Self::Scalar)
        }
    }

    pub fn detected() -> Self {
        if Self::Avx512F.is_supported() {
            return Self::Avx512F;
        }

        if Self::Avx2Fma.is_supported() {
            return Self::Avx2Fma;
        }

        if Self::Avx2.is_supported() {
            return Self::Avx2;
        }

        if Self::AvxFma.is_supported() {
            return Self::AvxFma;
        }

        if Self::Avx.is_supported() {
            return Self::Avx;
        }

        if Self::Sse2.is_supported() {
            return Self::Sse2;
        }

        Self::Scalar
    }

    fn resolve(requested: Option<Self>) -> Self {
        match requested {
            Some(backend) if backend.is_supported() => backend,
            _ => Self::detected(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GenerationError {
    NonFiniteCoordinate {
        index: usize,
    },
    CoordinateOutOfRange {
        index: usize,
    },
    OutputLengthMismatch {
        points: usize,
        output: usize,
    },
    InvalidProgram,
}

impl std::fmt::Display for GenerationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NonFiniteCoordinate { index } => {
                write!(f, "Non-finite coordinate at point {index}")
            }
            Self::CoordinateOutOfRange { index } => {
                write!(
                    f,
                    "Coordinate exceeds the supported noise lattice range at point {index}"
                )
            }
            Self::OutputLengthMismatch { points, output } => {
                write!(
                    f,
                    "Point count ({points}) does not match output length ({output})"
                )
            }
            Self::InvalidProgram => {
                write!(f, "The compiled expression program is invalid")
            }
        }
    }
}

impl std::error::Error for GenerationError {}

fn validate_program(preset: &CompiledPreset) -> Result<(), GenerationError> {
    let program = &preset.program;

    if program.instructions.is_empty()
        || program.register_count < 2
        || program.output as usize >= program.register_count
    {
        return Err(GenerationError::InvalidProgram);
    }

    let valid_register = |register: u16| {
        (register as usize) < program.register_count
    };

    for instruction in &program.instructions {
        let valid = match *instruction {
            Instruction::Input { dst, .. } => {
                valid_register(dst)
            }
            Instruction::Constant { dst, .. } => {
                valid_register(dst)
            }
            Instruction::Add { dst, a, b }
            | Instruction::Sub { dst, a, b }
            | Instruction::Mul { dst, a, b }
            | Instruction::Div { dst, a, b } => {
                valid_register(dst)
                    && valid_register(a)
                    && valid_register(b)
            }
            Instruction::Fbm { dst, noise } => {
                valid_register(dst)
                    && (noise as usize) < preset.noises.len()
            }
            Instruction::Ridge { dst, source }
            | Instruction::Abs { dst, source } => {
                valid_register(dst)
                    && valid_register(source)
            }
        };

        if !valid {
            return Err(GenerationError::InvalidProgram);
        }
    }

    Ok(())
}

fn validate_points<P: GenPoint>(
    points: &[P],
) -> Result<(), GenerationError> {
    for (index, point) in points.iter().copied().enumerate() {
        let point = point.into_dvec2();

        if !point.x.is_finite() || !point.y.is_finite() {
            return Err(GenerationError::NonFiniteCoordinate { index });
        }
    }

    Ok(())
}

fn execute_scalar_f32<P: GenPoint>(
    preset: &CompiledPreset,
    points: &[P],
    output: &mut [f32],
    index_offset: usize,
    registers: &mut [f32]
) -> Result<(), GenerationError> {
    let program = &preset.program;

    for (local_index, point) in points.iter().copied().enumerate() {
        let index = index_offset + local_index;
        for instruction in &program.instructions {
            match *instruction {
                Instruction::Input { dst, axis } => {
                    let (x, z) = point.coordinates_f32();

                    registers[dst as usize] = match axis {
                        Axis::X => x,
                        Axis::Z => z,
                    };
                }

                Instruction::Constant {
                    dst,
                    value_f32,
                    ..
                } => {
                    registers[dst as usize] = value_f32;
                }

                Instruction::Add { dst, a, b } => {
                    registers[dst as usize] =
                        registers[a as usize] + registers[b as usize];
                }

                Instruction::Sub { dst, a, b } => {
                    registers[dst as usize] =
                        registers[a as usize] - registers[b as usize];
                }

                Instruction::Mul { dst, a, b } => {
                    registers[dst as usize] =
                        registers[a as usize] * registers[b as usize];
                }

                Instruction::Div { dst, a, b } => {
                    registers[dst as usize] =
                        registers[a as usize] / registers[b as usize];
                }

                Instruction::Fbm { dst, noise } => {
                    registers[dst as usize] = point
                        .fbm_f32(&preset.noises[noise as usize])
                        .ok_or(
                            GenerationError::CoordinateOutOfRange { index },
                        )?;
                }

                Instruction::Ridge { dst, source } => {
                    registers[dst as usize] =
                        1.0 - registers[source as usize].abs();
                }

                Instruction::Abs { dst, source } => {
                    registers[dst as usize] =
                        registers[source as usize].abs();
                }
            }
        }

        output[index] = registers[program.output as usize];
    }

    Ok(())
}

fn execute_scalar_f64<P: GenPoint>(
    preset: &CompiledPreset,
    points: &[P],
    output: &mut [f64],
    index_offset: usize,
    registers: &mut [f64]
) -> Result<(), GenerationError> {
    let program = &preset.program;

    for (local_index, point) in points.iter().copied().enumerate() {
        let index = index_offset + local_index;
        let point64 = point.into_dvec2();
        for instruction in &program.instructions {
            match *instruction {
                Instruction::Input { dst, axis } => {
                    let (x, z) = point.coordinates_f64();

                    registers[dst as usize] = match axis {
                        Axis::X => x,
                        Axis::Z => z,
                    };
                }

                Instruction::Constant {
                    dst,
                    value_f64,
                    ..
                } => {
                    registers[dst as usize] = value_f64;
                }

                Instruction::Add { dst, a, b } => {
                    registers[dst as usize] =
                        registers[a as usize] + registers[b as usize];
                }

                Instruction::Sub { dst, a, b } => {
                    registers[dst as usize] =
                        registers[a as usize] - registers[b as usize];
                }

                Instruction::Mul { dst, a, b } => {
                    registers[dst as usize] =
                        registers[a as usize] * registers[b as usize];
                }

                Instruction::Div { dst, a, b } => {
                    registers[dst as usize] =
                        registers[a as usize] / registers[b as usize];
                }

                Instruction::Fbm { dst, noise } => {
                    registers[dst as usize] = fbm_f64(
                        &preset.noises[noise as usize],
                        point64,
                    )
                        .ok_or(
                            GenerationError::CoordinateOutOfRange { index },
                        )?;
                }

                Instruction::Ridge { dst, source } => {
                    registers[dst as usize] =
                        1.0 - registers[source as usize].abs();
                }

                Instruction::Abs { dst, source } => {
                    registers[dst as usize] =
                        registers[source as usize].abs();
                }
            }
        }

        output[index] = registers[program.output as usize];
    }

    Ok(())
}

#[inline]
fn load_point_batch_f32<P: GenPoint, const LANES: usize>(
    points: &[P],
    base: usize,
) -> ([f32; LANES], [f32; LANES]) {
    let mut xs = [0.0_f32; LANES];
    let mut zs = [0.0_f32; LANES];

    for lane in 0..LANES {
        let (x, z) = points[base + lane].coordinates_f32();
        xs[lane] = x;
        zs[lane] = z;
    }

    (xs, zs)
}

#[inline]
fn load_point_batch_f64<P: GenPoint, const LANES: usize>(
    points: &[P],
    base: usize,
) -> ([f64; LANES], [f64; LANES]) {
    let mut xs = [0.0_f64; LANES];
    let mut zs = [0.0_f64; LANES];

    for lane in 0..LANES {
        let (x, z) = points[base + lane].coordinates_f64();
        xs[lane] = x;
        zs[lane] = z;
    }

    (xs, zs)
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx")]
unsafe fn fbm_avx_batch(
    config: &NoiseConfig,
    x: __m256,
    z: __m256,
) -> __m256 {
    unsafe {
        let mut xs = [0.0_f32; 8];
        let mut zs = [0.0_f32; 8];
        let mut values = [0.0_f32; 8];

        _mm256_storeu_ps(xs.as_mut_ptr(), x);
        _mm256_storeu_ps(zs.as_mut_ptr(), z);

        for lane in 0..8 {
            values[lane] = fbm_scalar(
                config,
                Vec2::new(xs[lane], zs[lane]),
            );
        }

        _mm256_loadu_ps(values.as_ptr())
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx,fma")]
unsafe fn fbm_avx_fma_batch(
    config: &NoiseConfig,
    x: __m256,
    z: __m256,
) -> __m256 {
    unsafe {
        let mut xs = [0.0_f32; 8];
        let mut zs = [0.0_f32; 8];
        let mut values = [0.0_f32; 8];

        _mm256_storeu_ps(xs.as_mut_ptr(), x);
        _mm256_storeu_ps(zs.as_mut_ptr(), z);

        for lane in 0..8 {
            values[lane] = fbm_scalar_fma(
                config,
                xs[lane],
                zs[lane],
            );
        }

        _mm256_loadu_ps(values.as_ptr())
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
unsafe fn fbm_sse2_batch(
    config: &NoiseConfig,
    x: __m128,
    z: __m128,
) -> __m128 {
    unsafe {
        let mut xs = [0.0_f32; 4];
        let mut zs = [0.0_f32; 4];
        let mut values = [0.0_f32; 4];

        _mm_storeu_ps(xs.as_mut_ptr(), x);
        _mm_storeu_ps(zs.as_mut_ptr(), z);

        for lane in 0..4 {
            values[lane] = fbm_scalar(
                config,
                Vec2::new(xs[lane], zs[lane]),
            );
        }

        _mm_loadu_ps(values.as_ptr())
    }
}

#[cfg(target_arch = "x86_64")]
macro_rules! define_f32_simd_executor {
    (
        $name:ident,
        $feature:literal,
        $vector:ty,
        $lanes:expr,
        $load:ident,
        $store:ident,
        $zero:ident,
        $set1:ident,
        $add:ident,
        $sub:ident,
        $mul:ident,
        $div:ident,
        $andnot:ident,
        $fbm:ident
    ) => {
        #[target_feature(enable = $feature)]
        unsafe fn $name<P: GenPoint>(
            preset: &CompiledPreset,
            points: &[P],
            output: &mut [f32],
        ) -> Result<(), GenerationError> {
            unsafe {
                let program = &preset.program;

                let mut registers: Vec<$vector> =
                    vec![$zero(); program.register_count];

                let mut scalar_registers =
                    vec![0.0_f32; program.register_count];

                let vector_count =
                    points.len() / $lanes * $lanes;

                let mut base = 0;

                while base < vector_count {
                    let (xs, zs) =
                        load_point_batch_f32::<P, $lanes>(points, base);

                    let x_values = $load(xs.as_ptr());
                    let z_values = $load(zs.as_ptr());

                    for instruction in &program.instructions {
                        match *instruction {
                            Instruction::Input { dst, axis } => {
                                registers[dst as usize] = match axis {
                                    Axis::X => x_values,
                                    Axis::Z => z_values,
                                };
                            }

                            Instruction::Constant {
                                dst,
                                value_f32,
                                ..
                            } => {
                                registers[dst as usize] =
                                    $set1(value_f32);
                            }

                            Instruction::Add { dst, a, b } => {
                                registers[dst as usize] = $add(
                                    registers[a as usize],
                                    registers[b as usize],
                                );
                            }

                            Instruction::Sub { dst, a, b } => {
                                registers[dst as usize] = $sub(
                                    registers[a as usize],
                                    registers[b as usize],
                                );
                            }

                            Instruction::Mul { dst, a, b } => {
                                registers[dst as usize] = $mul(
                                    registers[a as usize],
                                    registers[b as usize],
                                );
                            }

                            Instruction::Div { dst, a, b } => {
                                registers[dst as usize] = $div(
                                    registers[a as usize],
                                    registers[b as usize],
                                );
                            }

                            Instruction::Fbm { dst, noise } => {
                                let config = &preset.noises[noise as usize];

                                registers[dst as usize] =
                                    if P::PRECISE_F32_FBM {
                                        let mut values =
                                            [0.0_f32; $lanes];

                                        for lane in 0..$lanes {
                                            values[lane] = points[base + lane]
                                                .fbm_f32(config)
                                                .ok_or(
                                                    GenerationError::CoordinateOutOfRange {
                                                        index: base + lane,
                                                    },
                                                )?;
                                        }

                                        $load(values.as_ptr())
                                    } else {
                                        $fbm(
                                            config,
                                            x_values,
                                            z_values,
                                        )
                                    };
                            }

                            Instruction::Ridge { dst, source } => {
                                let absolute = $andnot(
                                    $set1(-0.0),
                                    registers[source as usize],
                                );

                                registers[dst as usize] = $sub(
                                    $set1(1.0),
                                    absolute,
                                );
                            }

                            Instruction::Abs { dst, source } => {
                                registers[dst as usize] = $andnot(
                                    $set1(-0.0),
                                    registers[source as usize],
                                );
                            }
                        }
                    }

                    $store(
                        output.as_mut_ptr().add(base),
                        registers[program.output as usize],
                    );

                    base += $lanes;
                }

                if vector_count < points.len() {
                    execute_scalar_f32(
                        preset,
                        &points[vector_count..],
                        &mut output[vector_count..],
                        vector_count,
                        &mut scalar_registers,
                    )?;
                }

                Ok(())
            }
        }
    };
}

#[cfg(target_arch = "x86_64")]
define_f32_simd_executor!(
    execute_sse2_f32_inner,
    "sse2",
    __m128,
    4,
    _mm_loadu_ps,
    _mm_storeu_ps,
    _mm_setzero_ps,
    _mm_set1_ps,
    _mm_add_ps,
    _mm_sub_ps,
    _mm_mul_ps,
    _mm_div_ps,
    _mm_andnot_ps,
    fbm_sse2_batch
);

#[cfg(target_arch = "x86_64")]
define_f32_simd_executor!(
    execute_avx_f32_inner,
    "avx",
    __m256,
    8,
    _mm256_loadu_ps,
    _mm256_storeu_ps,
    _mm256_setzero_ps,
    _mm256_set1_ps,
    _mm256_add_ps,
    _mm256_sub_ps,
    _mm256_mul_ps,
    _mm256_div_ps,
    _mm256_andnot_ps,
    fbm_avx_batch
);

#[cfg(target_arch = "x86_64")]
define_f32_simd_executor!(
    execute_avx_fma_f32_inner,
    "avx,fma",
    __m256,
    8,
    _mm256_loadu_ps,
    _mm256_storeu_ps,
    _mm256_setzero_ps,
    _mm256_set1_ps,
    _mm256_add_ps,
    _mm256_sub_ps,
    _mm256_mul_ps,
    _mm256_div_ps,
    _mm256_andnot_ps,
    fbm_avx_fma_batch
);

#[cfg(target_arch = "x86_64")]
define_f32_simd_executor!(
    execute_avx2_f32_inner,
    "avx2",
    __m256,
    8,
    _mm256_loadu_ps,
    _mm256_storeu_ps,
    _mm256_setzero_ps,
    _mm256_set1_ps,
    _mm256_add_ps,
    _mm256_sub_ps,
    _mm256_mul_ps,
    _mm256_div_ps,
    _mm256_andnot_ps,
    fbm_avx2
);

#[cfg(target_arch = "x86_64")]
define_f32_simd_executor!(
    execute_avx2_fma_f32_inner,
    "avx2,fma",
    __m256,
    8,
    _mm256_loadu_ps,
    _mm256_storeu_ps,
    _mm256_setzero_ps,
    _mm256_set1_ps,
    _mm256_add_ps,
    _mm256_sub_ps,
    _mm256_mul_ps,
    _mm256_div_ps,
    _mm256_andnot_ps,
    fbm_avx2_fma
);

#[cfg(target_arch = "x86_64")]
define_f32_simd_executor!(
    execute_avx512_f32_inner,
    "avx512f",
    __m512,
    16,
    _mm512_loadu_ps,
    _mm512_storeu_ps,
    _mm512_setzero_ps,
    _mm512_set1_ps,
    _mm512_add_ps,
    _mm512_sub_ps,
    _mm512_mul_ps,
    _mm512_div_ps,
    _mm512_andnot_ps,
    fbm_avx512_fma
);

#[cfg(target_arch = "x86_64")]
macro_rules! define_f64_simd_executor {
    (
        $name:ident,
        $feature:literal,
        $vector:ty,
        $lanes:expr,
        $load:ident,
        $store:ident,
        $zero:ident,
        $set1:ident,
        $add:ident,
        $sub:ident,
        $mul:ident,
        $div:ident,
        $andnot:ident
    ) => {
        #[target_feature(enable = $feature)]
        unsafe fn $name<P: GenPoint>(
            preset: &CompiledPreset,
            points: &[P],
            output: &mut [f64],
        ) -> Result<(), GenerationError> {
            unsafe {
                let program = &preset.program;

                let mut registers: Vec<$vector> =
                    vec![$zero(); program.register_count];

                let mut scalar_registers =
                    vec![0.0_f64; program.register_count];

                let vector_count =
                    points.len() / $lanes * $lanes;

                let mut base = 0;

                while base < vector_count {
                    let (xs, zs) =
                        load_point_batch_f64::<P, $lanes>(points, base);

                    let x_values = $load(xs.as_ptr());
                    let z_values = $load(zs.as_ptr());

                    for instruction in &program.instructions {
                        match *instruction {
                            Instruction::Input { dst, axis } => {
                                registers[dst as usize] = match axis {
                                    Axis::X => x_values,
                                    Axis::Z => z_values,
                                };
                            }

                            Instruction::Constant {
                                dst,
                                value_f64,
                                ..
                            } => {
                                registers[dst as usize] =
                                    $set1(value_f64);
                            }

                            Instruction::Add { dst, a, b } => {
                                registers[dst as usize] = $add(
                                    registers[a as usize],
                                    registers[b as usize],
                                );
                            }

                            Instruction::Sub { dst, a, b } => {
                                registers[dst as usize] = $sub(
                                    registers[a as usize],
                                    registers[b as usize],
                                );
                            }

                            Instruction::Mul { dst, a, b } => {
                                registers[dst as usize] = $mul(
                                    registers[a as usize],
                                    registers[b as usize],
                                );
                            }

                            Instruction::Div { dst, a, b } => {
                                registers[dst as usize] = $div(
                                    registers[a as usize],
                                    registers[b as usize],
                                );
                            }

                            Instruction::Fbm { dst, noise } => {
                                let config = &preset.noises[noise as usize];
                                let mut values = [0.0_f64; $lanes];

                                for lane in 0..$lanes {
                                    values[lane] = fbm_f64(
                                        config,
                                        points[base + lane].into_dvec2(),
                                    )
                                    .ok_or(
                                        GenerationError::CoordinateOutOfRange {
                                            index: base + lane,
                                        },
                                    )?;
                                }

                                registers[dst as usize] =
                                    $load(values.as_ptr());
                            }

                            Instruction::Ridge { dst, source } => {
                                let absolute = $andnot(
                                    $set1(-0.0),
                                    registers[source as usize],
                                );

                                registers[dst as usize] = $sub(
                                    $set1(1.0),
                                    absolute,
                                );
                            }

                            Instruction::Abs { dst, source } => {
                                registers[dst as usize] = $andnot(
                                    $set1(-0.0),
                                    registers[source as usize],
                                );
                            }
                        }
                    }

                    $store(
                        output.as_mut_ptr().add(base),
                        registers[program.output as usize],
                    );

                    base += $lanes;
                }

                if vector_count < points.len() {
                    execute_scalar_f64(
                        preset,
                        &points[vector_count..],
                        &mut output[vector_count..],
                        vector_count,
                        &mut scalar_registers,
                    )?;
                }

                Ok(())
            }
        }
    };
}

#[cfg(target_arch = "x86_64")]
define_f64_simd_executor!(
    execute_sse2_f64_inner,
    "sse2",
    __m128d,
    2,
    _mm_loadu_pd,
    _mm_storeu_pd,
    _mm_setzero_pd,
    _mm_set1_pd,
    _mm_add_pd,
    _mm_sub_pd,
    _mm_mul_pd,
    _mm_div_pd,
    _mm_andnot_pd
);

#[cfg(target_arch = "x86_64")]
define_f64_simd_executor!(
    execute_avx_f64_inner,
    "avx",
    __m256d,
    4,
    _mm256_loadu_pd,
    _mm256_storeu_pd,
    _mm256_setzero_pd,
    _mm256_set1_pd,
    _mm256_add_pd,
    _mm256_sub_pd,
    _mm256_mul_pd,
    _mm256_div_pd,
    _mm256_andnot_pd
);

#[cfg(target_arch = "x86_64")]
define_f64_simd_executor!(
    execute_avx512_f64_inner,
    "avx512f",
    __m512d,
    8,
    _mm512_loadu_pd,
    _mm512_storeu_pd,
    _mm512_setzero_pd,
    _mm512_set1_pd,
    _mm512_add_pd,
    _mm512_sub_pd,
    _mm512_mul_pd,
    _mm512_div_pd,
    _mm512_andnot_pd
);

fn execute_f32<P: GenPoint>(
    preset: &CompiledPreset,
    points: &[P],
    output: &mut [f32],
    backend: GenBackend,
) -> Result<(), GenerationError> {
    match backend {
        GenBackend::Scalar => {
            let mut registers =
                vec![0.0_f32; preset.program.register_count];

            execute_scalar_f32(
                preset,
                points,
                output,
                0,
                &mut registers,
            )
        }

        #[cfg(target_arch = "x86_64")]
        GenBackend::Sse2 => unsafe {
            execute_sse2_f32_inner(preset, points, output)
        },

        #[cfg(target_arch = "x86_64")]
        GenBackend::Avx => unsafe {
            execute_avx_f32_inner(preset, points, output)
        },

        #[cfg(target_arch = "x86_64")]
        GenBackend::AvxFma => unsafe {
            execute_avx_fma_f32_inner(preset, points, output)
        },

        #[cfg(target_arch = "x86_64")]
        GenBackend::Avx2 => unsafe {
            execute_avx2_f32_inner(preset, points, output)
        },

        #[cfg(target_arch = "x86_64")]
        GenBackend::Avx2Fma => unsafe {
            execute_avx2_fma_f32_inner(preset, points, output)
        },

        #[cfg(target_arch = "x86_64")]
        GenBackend::Avx512F => unsafe {
            execute_avx512_f32_inner(preset, points, output)
        },
    }
}

fn execute_f64<P: GenPoint>(
    preset: &CompiledPreset,
    points: &[P],
    output: &mut [f64],
    backend: GenBackend,
) -> Result<(), GenerationError> {
    match backend {
        GenBackend::Scalar => {
            let mut registers =
                vec![0.0_f64; preset.program.register_count];

            execute_scalar_f64(
                preset,
                points,
                output,
                0,
                &mut registers,
            )
        }

        #[cfg(target_arch = "x86_64")]
        GenBackend::Sse2 => unsafe {
            execute_sse2_f64_inner(preset, points, output)
        },

        #[cfg(target_arch = "x86_64")]
        GenBackend::Avx512F => unsafe {
            execute_avx512_f64_inner(preset, points, output)
        },

        #[cfg(target_arch = "x86_64")]
        GenBackend::Avx
        | GenBackend::AvxFma
        | GenBackend::Avx2
        | GenBackend::Avx2Fma => unsafe {
            execute_avx_f64_inner(preset, points, output)
        },
    }
}

pub trait GenOutput<P: GenPoint>: sealed::Output<P> {
    fn execute_with_backend(
        preset: &CompiledPreset,
        points: &[P],
        output: &mut [Self],
        backend: GenBackend,
    ) -> Result<(), GenerationError>
    where
        Self: Sized;
}

impl sealed::Output<Vec2> for f32 {}

impl GenOutput<Vec2> for f32 {
    fn execute_with_backend(
        preset: &CompiledPreset,
        points: &[Vec2],
        output: &mut [f32],
        backend: GenBackend,
    ) -> Result<(), GenerationError> {
        execute_f32(preset, points, output, backend)
    }
}

impl sealed::Output<DVec2> for f32 {}

impl GenOutput<DVec2> for f32 {
    fn execute_with_backend(
        preset: &CompiledPreset,
        points: &[DVec2],
        output: &mut [f32],
        backend: GenBackend,
    ) -> Result<(), GenerationError> {
        execute_f32(preset, points, output, backend)
    }
}

impl sealed::Output<Vec2> for f64 {}

impl GenOutput<Vec2> for f64 {
    fn execute_with_backend(
        preset: &CompiledPreset,
        points: &[Vec2],
        output: &mut [f64],
        backend: GenBackend,
    ) -> Result<(), GenerationError> {
        execute_f64(preset, points, output, backend)
    }
}

impl sealed::Output<DVec2> for f64 {}

impl GenOutput<DVec2> for f64 {
    fn execute_with_backend(
        preset: &CompiledPreset,
        points: &[DVec2],
        output: &mut [f64],
        backend: GenBackend,
    ) -> Result<(), GenerationError> {
        execute_f64(preset, points, output, backend)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct GenWorkspace;

impl GenWorkspace {
    pub fn execute<P, O>(
        preset: &CompiledPreset,
        points: &[P],
        output: &mut [O],
        requested_backend: Option<GenBackend>,
    ) -> Result<GenBackend, GenerationError>
    where
        P: GenPoint,
        O: GenOutput<P>,
    {
        if points.len() != output.len() {
            return Err(GenerationError::OutputLengthMismatch {
                points: points.len(),
                output: output.len(),
            });
        }

        validate_program(preset)?;
        validate_points(points)?;

        let backend = GenBackend::resolve(requested_backend);

        O::execute_with_backend(
            preset,
            points,
            output,
            backend,
        )?;

        Ok(backend)
    }
    pub fn execute_muted<P, O>(
        preset: &CompiledPreset,
        points: &[P],
        output: &mut [O],
        requested_backend: Option<GenBackend>,
    ) where
        P: GenPoint,
        O: GenOutput<P>, {
        let _ = Self::execute(preset, points, output, requested_backend);
    }
}

pub fn instruction_count(program: &Program) -> usize {
    program.instructions.len()
}
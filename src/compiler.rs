
use std::collections::BTreeMap;
use crate::presets::{fbm_scalar, NoiseConfig};

#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;
#[cfg(target_arch = "x86_64")]
use crate::presets::{
    fbm_avx2,
    fbm_avx2_fma,
    fbm_avx512_fma,
    fbm_scalar_fma,
};

#[derive(Clone, Copy, Debug)]
enum Token {
    Number(f32),
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
                        .parse::<f32>()
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
    Number(f32),
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
    Constant { dst: u16, value: f32 },
    Add { dst: u16, a: u16, b: u16 },
    Sub { dst: u16, a: u16, b: u16 },
    Mul { dst: u16, a: u16, b: u16 },
    Div { dst: u16, a: u16, b: u16 },
    Fbm { dst: u16, noise: u16, x: u16, z: u16 },
    Ridge { dst: u16, source: u16 },
    Abs { dst: u16, source: u16 },
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
                self.instructions.push(Instruction::Constant { dst, value: *value });
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
                self.instructions.push(Instruction::Constant { dst: zero, value: 0.0 });
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
                    if args.len() != 3 {
                        return Err("fbm expects exactly three arguments: fbm(noise_name, x, z)".to_owned());
                    }
                    let noise_name = match &args[0] {
                        Expr::Name(name) => name,
                        _ => return Err("The first fbm argument must be a declared noise name".to_owned()),
                    };
                    let noise = self
                        .noises
                        .get(noise_name)
                        .copied()
                        .ok_or_else(|| format!("Noise source '{noise_name}' was not declared in YAML"))?;
                    let x = self.compile_expr(&args[1])?;
                    let z = self.compile_expr(&args[2])?;
                    let dst = self.allocate()?;
                    self.instructions.push(Instruction::Fbm { dst, noise, x, z });
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

pub struct ScalarWorkspace {
    registers: Vec<f32>,
}

impl ScalarWorkspace {
    pub fn new(program: &Program) -> Self {
        Self {
            registers: vec![0.0_f32; program.register_count],
        }
    }
}

pub fn execute_scalar_with_workspace(
    program: &Program,
    noises: &[NoiseConfig],
    xs: &[f32],
    zs: &[f32],
    output: &mut [f32],
    workspace: &mut ScalarWorkspace,
) {
    let count = xs.len().min(zs.len()).min(output.len());
    let registers = &mut workspace.registers;

    for index in 0..count {
        for instruction in &program.instructions {
            match *instruction {
                Instruction::Input { dst, axis } => {
                    registers[dst as usize] = match axis {
                        Axis::X => xs[index],
                        Axis::Z => zs[index],
                    };
                }
                Instruction::Constant { dst, value } => registers[dst as usize] = value,
                Instruction::Add { dst, a, b } => {
                    registers[dst as usize] = registers[a as usize] + registers[b as usize];
                }
                Instruction::Sub { dst, a, b } => {
                    registers[dst as usize] = registers[a as usize] - registers[b as usize];
                }
                Instruction::Mul { dst, a, b } => {
                    registers[dst as usize] = registers[a as usize] * registers[b as usize];
                }
                Instruction::Div { dst, a, b } => {
                    registers[dst as usize] = registers[a as usize] / registers[b as usize];
                }
                Instruction::Fbm { dst, noise, x, z } => {
                    registers[dst as usize] = fbm_scalar(
                        &noises[noise as usize],
                        registers[x as usize],
                        registers[z as usize],
                    );
                }
                Instruction::Ridge { dst, source } => {
                    registers[dst as usize] = 1.0 - registers[source as usize].abs();
                }
                Instruction::Abs { dst, source } => {
                    registers[dst as usize] = registers[source as usize].abs();
                }
            }
        }
        output[index] = registers[program.output as usize];
    }
}

pub fn execute_scalar(
    program: &Program,
    noises: &[NoiseConfig],
    xs: &[f32],
    zs: &[f32],
    output: &mut [f32],
) {
    let mut workspace = ScalarWorkspace::new(program);
    execute_scalar_with_workspace(program, noises, xs, zs, output, &mut workspace);
}


#[cfg(target_arch = "x86_64")]
pub fn create_avx2_workspace(program: &Program) -> Option<Avx2Workspace> {
    if !is_x86_feature_detected!("avx2") {
        return None;
    }

    Some(unsafe { create_avx2_workspace_inner(program) })
}

#[cfg(target_arch = "x86_64")]
pub unsafe fn execute_avx2_with_workspace_unchecked(
    program: &Program,
    noises: &[NoiseConfig],
    xs: &[f32],
    zs: &[f32],
    output: &mut [f32],
    workspace: &mut Avx2Workspace,
) {
    unsafe {
        execute_avx2_inner(
            program,
            noises,
            xs,
            zs,
            output,
            &mut workspace.registers,
            &mut workspace.scalar,
        );
    }
}

#[cfg(target_arch = "x86_64")]
pub fn execute_avx2_with_workspace(
    program: &Program,
    noises: &[NoiseConfig],
    xs: &[f32],
    zs: &[f32],
    output: &mut [f32],
    workspace: &mut Avx2Workspace,
) -> bool {
    if !is_x86_feature_detected!("avx2") {
        return false;
    }

    unsafe {
        execute_avx2_with_workspace_unchecked(
            program,
            noises,
            xs,
            zs,
            output,
            workspace,
        );
    }

    true
}

pub fn execute_avx2(
    program: &Program,
    noises: &[NoiseConfig],
    xs: &[f32],
    zs: &[f32],
    output: &mut [f32],
) -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        let Some(mut workspace) = create_avx2_workspace(program) else {
            return false;
        };

        execute_avx2_with_workspace(
            program,
            noises,
            xs,
            zs,
            output,
            &mut workspace,
        )
    }

    #[cfg(not(target_arch = "x86_64"))]
    {
        let _ = (program, noises, xs, zs, output);
        false
    }
}


pub fn instruction_count(program: &Program) -> usize {
    program.instructions.len()
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx")]
unsafe fn fbm_avx1_batch(
    config: &NoiseConfig,
    x: __m256,
    z: __m256,
) -> __m256 {
    unsafe {
        let mut xs = [0.0_f32; 8];
        let mut zs = [0.0_f32; 8];
        let mut result = [0.0_f32; 8];

        _mm256_storeu_ps(xs.as_mut_ptr(), x);
        _mm256_storeu_ps(zs.as_mut_ptr(), z);

        for lane in 0..8 {
            result[lane] = fbm_scalar(config, xs[lane], zs[lane]);
        }

        _mm256_loadu_ps(result.as_ptr())
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx,fma")]
unsafe fn fbm_avx1_fma_batch(
    config: &NoiseConfig,
    x: __m256,
    z: __m256,
) -> __m256 {
    unsafe {
        let mut xs = [0.0_f32; 8];
        let mut zs = [0.0_f32; 8];
        let mut result = [0.0_f32; 8];

        _mm256_storeu_ps(xs.as_mut_ptr(), x);
        _mm256_storeu_ps(zs.as_mut_ptr(), z);

        for lane in 0..8 {
            result[lane] = fbm_scalar_fma(config, xs[lane], zs[lane]);
        }

        _mm256_loadu_ps(result.as_ptr())
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
        let mut result = [0.0_f32; 4];

        _mm_storeu_ps(xs.as_mut_ptr(), x);
        _mm_storeu_ps(zs.as_mut_ptr(), z);

        for lane in 0..4 {
            result[lane] = fbm_scalar(config, xs[lane], zs[lane]);
        }

        _mm_loadu_ps(result.as_ptr())
    }
}
#[cfg(target_arch = "x86_64")]
macro_rules! define_simd_executor {
    (
        $name:ident,
        $feature:literal,
        $vector:ty,
        $lanes:expr,
        $load:ident,
        $store:ident,
        $set1:ident,
        $add:ident,
        $sub:ident,
        $mul:ident,
        $div:ident,
        $andnot:ident,
        $fbm:ident
    ) => {
        #[target_feature(enable = $feature)]
        unsafe fn $name(
            program: &Program,
            noises: &[NoiseConfig],
            xs: &[f32],
            zs: &[f32],
            output: &mut [f32],
            registers: &mut [$vector],
            scalar_workspace: &mut ScalarWorkspace,
        ) {
            let count = xs.len().min(zs.len()).min(output.len());
            let vector_count = count / $lanes * $lanes;
            let mut base = 0;

            while base < vector_count {
                unsafe {
                    let x_values = $load(xs.as_ptr().add(base));
                    let z_values = $load(zs.as_ptr().add(base));

                    for instruction in &program.instructions {
                        match *instruction {
                            Instruction::Input { dst, axis } => {
                                registers[dst as usize] = match axis {
                                    Axis::X => x_values,
                                    Axis::Z => z_values,
                                };
                            }

                            Instruction::Constant { dst, value } => {
                                registers[dst as usize] = $set1(value);
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

                            Instruction::Fbm { dst, noise, x, z } => {
                                registers[dst as usize] = $fbm(
                                    &noises[noise as usize],
                                    registers[x as usize],
                                    registers[z as usize],
                                );
                            }

                            Instruction::Ridge { dst, source } => {
                                let sign = $set1(-0.0);
                                let absolute = $andnot(
                                    sign,
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
                }

                base += $lanes;
            }

            if vector_count < count {
                execute_scalar_with_workspace(
                    program,
                    noises,
                    &xs[vector_count..count],
                    &zs[vector_count..count],
                    &mut output[vector_count..count],
                    scalar_workspace,
                );
            }
        }
    };
}
#[cfg(target_arch = "x86_64")]
define_simd_executor!(
    execute_avx512_inner,
    "avx512f",
    __m512,
    16,
    _mm512_loadu_ps,
    _mm512_storeu_ps,
    _mm512_set1_ps,
    _mm512_add_ps,
    _mm512_sub_ps,
    _mm512_mul_ps,
    _mm512_div_ps,
    _mm512_andnot_ps,
    fbm_avx512_fma
);

#[cfg(target_arch = "x86_64")]
define_simd_executor!(
    execute_avx2_fma_inner,
    "avx2,fma",
    __m256,
    8,
    _mm256_loadu_ps,
    _mm256_storeu_ps,
    _mm256_set1_ps,
    _mm256_add_ps,
    _mm256_sub_ps,
    _mm256_mul_ps,
    _mm256_div_ps,
    _mm256_andnot_ps,
    fbm_avx2_fma
);

#[cfg(target_arch = "x86_64")]
define_simd_executor!(
    execute_avx2_inner,
    "avx2",
    __m256,
    8,
    _mm256_loadu_ps,
    _mm256_storeu_ps,
    _mm256_set1_ps,
    _mm256_add_ps,
    _mm256_sub_ps,
    _mm256_mul_ps,
    _mm256_div_ps,
    _mm256_andnot_ps,
    fbm_avx2
);

#[cfg(target_arch = "x86_64")]
define_simd_executor!(
    execute_avx1_fma_inner,
    "avx,fma",
    __m256,
    8,
    _mm256_loadu_ps,
    _mm256_storeu_ps,
    _mm256_set1_ps,
    _mm256_add_ps,
    _mm256_sub_ps,
    _mm256_mul_ps,
    _mm256_div_ps,
    _mm256_andnot_ps,
    fbm_avx1_fma_batch
);

#[cfg(target_arch = "x86_64")]
define_simd_executor!(
    execute_avx1_inner,
    "avx",
    __m256,
    8,
    _mm256_loadu_ps,
    _mm256_storeu_ps,
    _mm256_set1_ps,
    _mm256_add_ps,
    _mm256_sub_ps,
    _mm256_mul_ps,
    _mm256_div_ps,
    _mm256_andnot_ps,
    fbm_avx1_batch
);

#[cfg(target_arch = "x86_64")]
define_simd_executor!(
    execute_sse2_inner,
    "sse2",
    __m128,
    4,
    _mm_loadu_ps,
    _mm_storeu_ps,
    _mm_set1_ps,
    _mm_add_ps,
    _mm_sub_ps,
    _mm_mul_ps,
    _mm_div_ps,
    _mm_andnot_ps,
    fbm_sse2_batch
);
#[cfg(target_arch = "x86_64")]
pub struct Sse2Workspace {
    registers: Vec<__m128>,
    scalar: ScalarWorkspace,
}

#[cfg(target_arch = "x86_64")]
pub struct AvxWorkspace {
    registers: Vec<__m256>,
    scalar: ScalarWorkspace,
}

#[cfg(target_arch = "x86_64")]
pub type Avx2Workspace = AvxWorkspace;

#[cfg(target_arch = "x86_64")]
pub struct Avx512Workspace {
    registers: Vec<__m512>,
    scalar: ScalarWorkspace,
}
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
unsafe fn create_sse2_workspace_inner(program: &Program) -> Sse2Workspace {
    let mut registers = Vec::with_capacity(program.register_count);

    for _ in 0..program.register_count {
        registers.push(_mm_setzero_ps());
    }

    Sse2Workspace {
        registers,
        scalar: ScalarWorkspace::new(program),
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx")]
unsafe fn create_avx_workspace_inner(program: &Program) -> AvxWorkspace {
    let mut registers = Vec::with_capacity(program.register_count);

    for _ in 0..program.register_count {
        registers.push(_mm256_setzero_ps());
    }

    AvxWorkspace {
        registers,
        scalar: ScalarWorkspace::new(program),
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn create_avx2_workspace_inner(program: &Program) -> Avx2Workspace {
    let mut registers = Vec::with_capacity(program.register_count);

    for _ in 0..program.register_count {
        registers.push(_mm256_setzero_ps());
    }

    Avx2Workspace {
        registers,
        scalar: ScalarWorkspace::new(program),
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f")]
unsafe fn create_avx512_workspace_inner(program: &Program) -> Avx512Workspace {
    let mut registers = Vec::with_capacity(program.register_count);

    for _ in 0..program.register_count {
        registers.push(_mm512_setzero_ps());
    }

    Avx512Workspace {
        registers,
        scalar: ScalarWorkspace::new(program),
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    pub fn detected() -> GenBackend {
        #[cfg(target_arch = "x86_64")]
        {
            if is_x86_feature_detected!("avx512f") {
                return GenBackend::Avx512F;
            }

            if is_x86_feature_detected!("avx2")
                && is_x86_feature_detected!("fma")
            {
                return GenBackend::Avx2Fma;
            }

            if is_x86_feature_detected!("avx2") {
                return GenBackend::Avx2;
            }

            if is_x86_feature_detected!("avx")
                && is_x86_feature_detected!("fma")
            {
                return GenBackend::AvxFma;
            }

            if is_x86_feature_detected!("avx") {
                return GenBackend::Avx;
            }

            if is_x86_feature_detected!("sse2") {
                return GenBackend::Sse2;
            }
        }

        GenBackend::Scalar
    }
}
pub enum GenWorkspace {
    Scalar(ScalarWorkspace),

    #[cfg(target_arch = "x86_64")]
    Sse2(Sse2Workspace),

    #[cfg(target_arch = "x86_64")]
    Avx(AvxWorkspace),

    #[cfg(target_arch = "x86_64")]
    AvxFma(AvxWorkspace),

    #[cfg(target_arch = "x86_64")]
    Avx2(Avx2Workspace),

    #[cfg(target_arch = "x86_64")]
    Avx2Fma(Avx2Workspace),

    #[cfg(target_arch = "x86_64")]
    Avx512(Avx512Workspace),
}

impl GenWorkspace {
    pub fn backend(&self) -> GenBackend {
        match self {
            Self::Scalar(_) => GenBackend::Scalar,

            #[cfg(target_arch = "x86_64")]
            Self::Sse2(_) => GenBackend::Sse2,

            #[cfg(target_arch = "x86_64")]
            Self::Avx(_) => GenBackend::Avx,

            #[cfg(target_arch = "x86_64")]
            Self::AvxFma(_) => GenBackend::AvxFma,

            #[cfg(target_arch = "x86_64")]
            Self::Avx2(_) => GenBackend::Avx2,

            #[cfg(target_arch = "x86_64")]
            Self::Avx2Fma(_) => GenBackend::Avx2Fma,

            #[cfg(target_arch = "x86_64")]
            Self::Avx512(_) => GenBackend::Avx512F,
        }
    }
    pub fn new(program: &Program) -> GenWorkspace {
        #[cfg(target_arch = "x86_64")]
        {
            match GenBackend::detected() {
                GenBackend::Avx512F => {
                    GenWorkspace::Avx512(
                        unsafe { create_avx512_workspace_inner(program) }
                    )
                }

                GenBackend::Avx2Fma => {
                    GenWorkspace::Avx2Fma(
                        unsafe { create_avx2_workspace_inner(program) }
                    )
                }

                GenBackend::Avx2 => {
                    GenWorkspace::Avx2(
                        unsafe { create_avx2_workspace_inner(program) }
                    )
                }

                GenBackend::AvxFma => {
                    GenWorkspace::AvxFma(
                        unsafe { create_avx_workspace_inner(program) }
                    )
                }

                GenBackend::Avx => {
                    GenWorkspace::Avx(
                        unsafe { create_avx_workspace_inner(program) }
                    )
                }

                GenBackend::Sse2 => {
                    GenWorkspace::Sse2(
                        unsafe { create_sse2_workspace_inner(program) }
                    )
                }

                GenBackend::Scalar => {
                    GenWorkspace::Scalar(ScalarWorkspace::new(program))
                }
            }
        }

        #[cfg(not(target_arch = "x86_64"))]
        {
            GenWorkspace::Scalar(ScalarWorkspace::new(program))
        }
    }
    pub fn execute(
        &mut self,
        program: &Program,
        noises: &[NoiseConfig],
        xs: &[f32],
        zs: &[f32],
        output: &mut [f32]
    ) {
        match self {
            GenWorkspace::Scalar(scalar) => {
                execute_scalar_with_workspace(
                    program,
                    noises,
                    xs,
                    zs,
                    output,
                    scalar,
                );
            }

            #[cfg(target_arch = "x86_64")]
            GenWorkspace::Sse2(ws) => unsafe {
                execute_sse2_inner(
                    program,
                    noises,
                    xs,
                    zs,
                    output,
                    &mut ws.registers,
                    &mut ws.scalar,
                );
            },

            #[cfg(target_arch = "x86_64")]
            GenWorkspace::Avx(ws) => unsafe {
                execute_avx1_inner(
                    program,
                    noises,
                    xs,
                    zs,
                    output,
                    &mut ws.registers,
                    &mut ws.scalar,
                );
            },

            #[cfg(target_arch = "x86_64")]
            GenWorkspace::AvxFma(ws) => unsafe {
                execute_avx1_fma_inner(
                    program,
                    noises,
                    xs,
                    zs,
                    output,
                    &mut ws.registers,
                    &mut ws.scalar,
                );
            },

            #[cfg(target_arch = "x86_64")]
            GenWorkspace::Avx2(ws) => unsafe {
                execute_avx2_inner(
                    program,
                    noises,
                    xs,
                    zs,
                    output,
                    &mut ws.registers,
                    &mut ws.scalar,
                );
            },

            #[cfg(target_arch = "x86_64")]
            GenWorkspace::Avx2Fma(ws) => unsafe {
                execute_avx2_fma_inner(
                    program,
                    noises,
                    xs,
                    zs,
                    output,
                    &mut ws.registers,
                    &mut ws.scalar,
                );
            },

            #[cfg(target_arch = "x86_64")]
            GenWorkspace::Avx512(ws) => unsafe {
                execute_avx512_inner(
                    program,
                    noises,
                    xs,
                    zs,
                    output,
                    &mut ws.registers,
                    &mut ws.scalar,
                );
            },
        }
    }
}

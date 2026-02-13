use crate::ast::{Atom, Term, UnitExpr};
use crate::db::UnitValueDef;
use crate::error::{Error, Result};
use crate::parser;
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{One, Signed, ToPrimitive, Zero};
use rust_decimal::Decimal;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DimensionVector(pub [i32; 8]);

impl DimensionVector {
    pub const ZERO: Self = Self([0; 8]);

    pub const LENGTH: Self = Self([1, 0, 0, 0, 0, 0, 0, 0]);
    pub const MASS: Self = Self([0, 1, 0, 0, 0, 0, 0, 0]);
    pub const TIME: Self = Self([0, 0, 1, 0, 0, 0, 0, 0]);
    pub const ANGLE: Self = Self([0, 0, 0, 1, 0, 0, 0, 0]);
    pub const TEMPERATURE: Self = Self([0, 0, 0, 0, 1, 0, 0, 0]);
    pub const CHARGE: Self = Self([0, 0, 0, 0, 0, 1, 0, 0]);
    pub const LUMINOUS_INTENSITY: Self = Self([0, 0, 0, 0, 0, 0, 1, 0]);
    pub const AMOUNT: Self = Self([0, 0, 0, 0, 0, 0, 0, 1]);

    pub fn from_ucum_dim(dim: &str) -> Option<Self> {
        match dim {
            "L" => Some(Self::LENGTH),
            "M" => Some(Self::MASS),
            "T" => Some(Self::TIME),
            "A" => Some(Self::ANGLE),
            "C" => Some(Self::TEMPERATURE),
            "Q" => Some(Self::CHARGE),
            "F" => Some(Self::LUMINOUS_INTENSITY),
            _ => None,
        }
    }

    pub fn add_scaled(&mut self, other: DimensionVector, k: i32) {
        for (dst, src) in self.0.iter_mut().zip(other.0.iter()) {
            *dst += src * k;
        }
    }
}

impl Hash for DimensionVector {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unit {
    pub dimensions: DimensionVector,
    pub kind: UnitKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnitKind {
    Multiplicative {
        factor: BigRational,
    },
    /// Base-value conversion: `base = value * factor + offset`.
    Affine {
        factor: BigRational,
        offset: BigRational,
    },
    NonLinear,
}

impl Unit {
    pub fn parse(expr: &str) -> Result<Self> {
        let ast = parser::parse(expr)?;
        resolve_expr(&ast)
    }

    pub fn to_base(&self, value: &BigRational) -> Result<BigRational> {
        match &self.kind {
            UnitKind::Multiplicative { factor } => Ok(value * factor),
            UnitKind::Affine { factor, offset } => Ok(value * factor + offset),
            UnitKind::NonLinear => Err(Error::NonLinear("<non-linear>".into())),
        }
    }

    pub fn from_base(&self, base: &BigRational) -> Result<BigRational> {
        match &self.kind {
            UnitKind::Multiplicative { factor } => Ok(base / factor),
            UnitKind::Affine { factor, offset } => Ok((base - offset) / factor),
            UnitKind::NonLinear => Err(Error::NonLinear("<non-linear>".into())),
        }
    }
}

pub fn equivalent(a: &str, b: &str) -> Result<bool> {
    let ua = Unit::parse(a)?;
    let ub = Unit::parse(b)?;
    if ua.dimensions != ub.dimensions {
        return Ok(false);
    }
    match (&ua.kind, &ub.kind) {
        (UnitKind::Multiplicative { .. }, UnitKind::Multiplicative { .. }) => Ok(true),
        _ => Ok(false),
    }
}

pub fn convertible(a: &str, b: &str) -> Result<bool> {
    let ua = Unit::parse(a)?;
    let ub = Unit::parse(b)?;
    if ua.dimensions != ub.dimensions {
        return Ok(false);
    }
    match (&ua.kind, &ub.kind) {
        (UnitKind::NonLinear, _) | (_, UnitKind::NonLinear) => Ok(false),
        _ => Ok(true),
    }
}

pub fn compare_decimal_quantities(
    left_value: &Decimal,
    left_unit: &str,
    right_value: &Decimal,
    right_unit: &str,
) -> Result<Ordering> {
    let lu = Unit::parse(left_unit)?;
    let ru = Unit::parse(right_unit)?;
    if lu.dimensions != ru.dimensions {
        return Err(Error::Incompatible {
            from: left_unit.into(),
            to: right_unit.into(),
        });
    }

    if matches!(lu.kind, UnitKind::NonLinear) || matches!(ru.kind, UnitKind::NonLinear) {
        return Err(Error::NonLinear(format!("{left_unit} vs {right_unit}")));
    }

    let lv = decimal_to_rational(*left_value)?;
    let rv = decimal_to_rational(*right_value)?;
    let lb = lu.to_base(&lv)?;
    let rb = ru.to_base(&rv)?;
    Ok(lb.cmp(&rb))
}

pub fn convert_decimal(value: Decimal, from: &str, to: &str) -> Result<Decimal> {
    let from_u = Unit::parse(from)?;
    let to_u = Unit::parse(to)?;
    if from_u.dimensions != to_u.dimensions {
        return Err(Error::Incompatible {
            from: from.into(),
            to: to.into(),
        });
    }

    let v = decimal_to_rational(value)?;
    let base = from_u.to_base(&v)?;
    let out = to_u.from_base(&base)?;
    rational_to_decimal(out)
}

fn resolve_expr(expr: &UnitExpr) -> Result<Unit> {
    let mut state = ResolveState::default();
    let num = resolve_factors(&mut state, &expr.numerator)?;
    let den = resolve_factors(&mut state, &expr.denominator)?;

    match (&num.kind, &den.kind) {
        (UnitKind::NonLinear, _) | (_, UnitKind::NonLinear) => {
            return Ok(Unit {
                dimensions: num.dimensions,
                kind: UnitKind::NonLinear,
            });
        }
        _ => {}
    }

    // Affine units cannot participate in products/quotients.
    if matches!(num.kind, UnitKind::Affine { .. })
        || matches!(den.kind, UnitKind::Affine { .. })
        || state.saw_affine
    {
        if expr.denominator.is_empty() && expr.numerator.len() == 1 {
            return Ok(num);
        }
        return Err(Error::Syntax {
            pos: 0,
            message: "affine units cannot be combined",
        });
    }

    let (mut dims, mut factor) = match num.kind {
        UnitKind::Multiplicative { factor } => (num.dimensions, factor),
        UnitKind::Affine { .. } => unreachable!(),
        UnitKind::NonLinear => unreachable!(),
    };

    let den_factor = match den.kind {
        UnitKind::Multiplicative { factor } => factor,
        UnitKind::Affine { .. } => unreachable!(),
        UnitKind::NonLinear => unreachable!(),
    };
    dims.add_scaled(den.dimensions, -1);
    factor /= den_factor;

    Ok(Unit {
        dimensions: dims,
        kind: UnitKind::Multiplicative { factor },
    })
}

#[derive(Default)]
struct ResolveState {
    memo: HashMap<String, Unit>,
    saw_affine: bool,
    prefix_codes: Option<Vec<String>>,
}

fn resolve_factors(state: &mut ResolveState, factors: &[(Term, i32)]) -> Result<Unit> {
    let mut dims = DimensionVector::ZERO;
    let mut factor = BigRational::one();
    for (term, exp) in factors {
        let unit = resolve_term(state, term)?;
        if *exp == 0 {
            continue;
        }
        match unit.kind {
            UnitKind::Affine { .. } => {
                if *exp != 1 {
                    return Err(Error::AffineExponent(format!("{term:?}")));
                }
                state.saw_affine = true;
                return Ok(unit);
            }
            UnitKind::NonLinear => {
                return Ok(unit);
            }
            UnitKind::Multiplicative { factor: u_factor } => {
                dims.add_scaled(unit.dimensions, *exp);
                factor *= pow_rational(&u_factor, *exp)?;
            }
        }
    }
    Ok(Unit {
        dimensions: dims,
        kind: UnitKind::Multiplicative { factor },
    })
}

fn resolve_term(state: &mut ResolveState, term: &Term) -> Result<Unit> {
    match term {
        Term::Atom(Atom::Integer(n)) => Ok(Unit {
            dimensions: DimensionVector::ZERO,
            kind: UnitKind::Multiplicative {
                factor: BigRational::from_integer(BigInt::from(*n)),
            },
        }),
        Term::Atom(Atom::Symbol(s)) => resolve_symbol(state, s),
        Term::Group(g) => resolve_expr(g),
    }
}

fn resolve_symbol(state: &mut ResolveState, symbol: &str) -> Result<Unit> {
    if symbol == "1" {
        return Ok(Unit {
            dimensions: DimensionVector::ZERO,
            kind: UnitKind::Multiplicative {
                factor: BigRational::one(),
            },
        });
    }

    if let Some(u) = state.memo.get(symbol) {
        return Ok(u.clone());
    }

    let db = crate::db();
    if let Some(dim) = db.base_units.get(symbol) {
        let u = Unit {
            dimensions: *dim,
            kind: UnitKind::Multiplicative {
                factor: BigRational::one(),
            },
        };
        state.memo.insert(symbol.into(), u.clone());
        return Ok(u);
    }

    if let Some(def) = db.units.get(symbol) {
        let u = resolve_unit_def(def)?;
        state.memo.insert(symbol.into(), u.clone());
        return Ok(u);
    }

    // Try prefix splitting (longest prefix wins).
    let prefix_codes = state
        .prefix_codes
        .get_or_insert_with(|| {
            let mut v: Vec<String> = db.prefixes.keys().cloned().collect();
            v.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
            v
        })
        .clone();

    for p in prefix_codes {
        if let Some(rest) = symbol.strip_prefix(&p) {
            if rest.is_empty() {
                continue;
            }

            let Some(prefix_factor) = db.prefixes.get(&p) else {
                continue;
            };

            // Prefer exact match, only then split.
            let base = if let Some(dim) = db.base_units.get(rest) {
                Unit {
                    dimensions: *dim,
                    kind: UnitKind::Multiplicative {
                        factor: BigRational::one(),
                    },
                }
            } else if let Some(def) = db.units.get(rest) {
                if !def.is_metric || def.is_special {
                    return Err(Error::NotPrefixable(rest.to_string()));
                }
                resolve_unit_def(def)?
            } else {
                continue;
            };

            match base.kind {
                UnitKind::Multiplicative { factor } => {
                    let u = Unit {
                        dimensions: base.dimensions,
                        kind: UnitKind::Multiplicative {
                            factor: factor * prefix_factor,
                        },
                    };
                    state.memo.insert(symbol.into(), u.clone());
                    return Ok(u);
                }
                _ => return Err(Error::NotPrefixable(rest.to_string())),
            }
        }
    }

    Err(Error::UnknownUnit(symbol.into()))
}

fn resolve_unit_def(def: &crate::db::UnitDef) -> Result<Unit> {
    match &def.def {
        UnitValueDef::Base => Ok(Unit {
            dimensions: DimensionVector::ZERO,
            kind: UnitKind::Multiplicative {
                factor: BigRational::one(),
            },
        }),
        UnitValueDef::Linear { factor, unit } => {
            let inner = Unit::parse(unit)?;
            match inner.kind {
                UnitKind::Multiplicative {
                    factor: inner_factor,
                } => Ok(Unit {
                    dimensions: inner.dimensions,
                    kind: UnitKind::Multiplicative {
                        factor: inner_factor * factor,
                    },
                }),
                UnitKind::Affine { .. } => Err(Error::Syntax {
                    pos: 0,
                    message: "affine units cannot be scaled",
                }),
                UnitKind::NonLinear => Ok(Unit {
                    dimensions: inner.dimensions,
                    kind: UnitKind::NonLinear,
                }),
            }
        }
        UnitValueDef::Function { name, value, unit } => {
            let inner = Unit::parse(unit)?;
            let (dims, scale) = match inner.kind {
                UnitKind::Multiplicative {
                    factor: inner_factor,
                } => (inner.dimensions, inner_factor),
                _ => {
                    return Err(Error::Db(format!(
                        "function unit '{}' has non-multiplicative base '{unit}'",
                        def.code
                    )));
                }
            };

            let scale = scale * value;
            let offset = match name.as_str() {
                // Degree Celsius: K = Cel + 273.15
                "Cel" => decimal_string_to_rational("273.15")?,
                // Degree Fahrenheit: K = (degF + 459.67) * 5/9
                "degF" => {
                    let o = decimal_string_to_rational("459.67")?;
                    &o * &scale
                }
                // Degree Reaumur: K = degRe * 5/4 + 273.15
                "degRe" => decimal_string_to_rational("273.15")?,
                _ => {
                    return Err(Error::NonLinear(def.code.clone()));
                }
            };

            Ok(Unit {
                dimensions: dims,
                kind: UnitKind::Affine {
                    factor: scale,
                    offset,
                },
            })
        }
        UnitValueDef::NonLinear {
            _name: _,
            _value: _,
            unit,
        } => {
            let inner = Unit::parse(unit)?;
            Ok(Unit {
                dimensions: inner.dimensions,
                kind: UnitKind::NonLinear,
            })
        }
    }
}

fn pow_rational(x: &BigRational, exp: i32) -> Result<BigRational> {
    if exp == 0 {
        return Ok(BigRational::one());
    }
    let exp_u: u32 = exp.try_into().map_err(|_| Error::Overflow)?;
    Ok(pow_rational_u32(x, exp_u))
}

fn pow_rational_u32(x: &BigRational, mut exp: u32) -> BigRational {
    let mut base = x.clone();
    let mut out = BigRational::one();
    while exp > 0 {
        if exp & 1 == 1 {
            out *= &base;
        }
        exp >>= 1;
        if exp > 0 {
            base = &base * &base;
        }
    }
    out
}

pub(crate) fn decimal_to_rational(d: Decimal) -> Result<BigRational> {
    let scale = d.scale();
    let mantissa = d.mantissa();
    let num = BigInt::from(mantissa);
    let den = BigInt::from(10u8).pow(scale);
    Ok(BigRational::new(num, den))
}

pub(crate) fn rational_to_decimal(r: BigRational) -> Result<Decimal> {
    let (num, den) = (r.numer().clone(), r.denom().clone());

    // Exact conversion when the denominator has no primes other than 2 and 5.
    if let Some((scale, mul)) = decimal_mul_for_den(&den) {
        if scale <= 28 {
            let scaled = num * mul;
            if let Some(n) = scaled.to_i128() {
                return Ok(Decimal::from_i128_with_scale(n, scale));
            }
        }
    }

    // Fallback to f64.
    let f = r.to_f64().ok_or(Error::Overflow)?;
    Decimal::from_f64_retain(f).ok_or(Error::Overflow)
}

fn decimal_mul_for_den(den: &BigInt) -> Option<(u32, BigInt)> {
    if den.is_zero() || !den.is_positive() {
        return None;
    }
    if *den == BigInt::one() {
        return Some((0, BigInt::one()));
    }

    let mut d = den.clone();
    let two = BigInt::from(2u8);
    let five = BigInt::from(5u8);

    let mut twos: u32 = 0;
    while (&d % &two).is_zero() {
        d /= &two;
        twos += 1;
    }

    let mut fives: u32 = 0;
    while (&d % &five).is_zero() {
        d /= &five;
        fives += 1;
    }

    if d != BigInt::one() {
        return None;
    }

    let scale = twos.max(fives);
    let pow_two = scale - twos;
    let pow_five = scale - fives;
    let mul = two.pow(pow_two) * five.pow(pow_five);
    Some((scale, mul))
}

fn decimal_string_to_rational(s: &str) -> Result<BigRational> {
    // `rust_decimal` parses finite decimals, which we can then lift to a rational.
    let d: Decimal = s
        .parse()
        .map_err(|_| Error::Db(format!("bad decimal '{s}'")))?;
    decimal_to_rational(d)
}

use crate::error::{Error, Result};
use crate::unit::DimensionVector;
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::One;
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct UcumDb {
    pub prefixes: HashMap<String, BigRational>,
    pub base_units: HashMap<String, DimensionVector>,
    pub units: HashMap<String, UnitDef>,
}

#[derive(Clone, Debug)]
pub struct UnitDef {
    pub code: String,
    pub is_metric: bool,
    pub is_special: bool,
    pub is_arbitrary: bool,
    pub class: Option<String>,
    pub def: UnitValueDef,
}

#[derive(Clone, Debug)]
pub enum UnitValueDef {
    Base,
    Linear {
        factor: BigRational,
        unit: String,
    },
    Function {
        name: String,
        value: BigRational,
        unit: String,
    },
    NonLinear {
        _name: String,
        _value: BigRational,
        unit: String,
    },
}

impl UcumDb {
    pub fn from_essence_xml(xml: &str) -> Result<Self> {
        let mut reader = Reader::from_str(xml);
        reader.trim_text(true);

        let mut prefixes: HashMap<String, BigRational> = HashMap::new();
        let mut base_units: HashMap<String, DimensionVector> = HashMap::new();
        let mut units: HashMap<String, UnitDef> = HashMap::new();

        #[derive(Default)]
        struct PrefixBuilder {
            code: String,
            value: Option<BigRational>,
        }

        #[derive(Default)]
        struct UnitBuilder {
            code: String,
            is_metric: bool,
            is_special: bool,
            is_arbitrary: bool,
            class: Option<String>,
            value_factor: Option<BigRational>,
            value_unit: Option<String>,
            function: Option<(String, BigRational, String)>,
        }

        let mut cur_prefix: Option<PrefixBuilder> = None;
        let mut cur_unit: Option<UnitBuilder> = None;

        let mut buf = Vec::new();
        loop {
            match reader
                .read_event_into(&mut buf)
                .map_err(|e| Error::Db(e.to_string()))?
            {
                Event::Start(e) => match e.name().as_ref() {
                    b"prefix" => {
                        let code = attr(&e, b"Code")?
                            .ok_or_else(|| Error::Db("prefix without Code".into()))?;
                        cur_prefix = Some(PrefixBuilder { code, value: None });
                    }
                    b"base-unit" => {
                        let code = attr(&e, b"Code")?
                            .ok_or_else(|| Error::Db("base-unit without Code".into()))?;
                        let dim = attr(&e, b"dim")?
                            .ok_or_else(|| Error::Db("base-unit without dim".into()))?;
                        let dv = DimensionVector::from_ucum_dim(&dim).ok_or_else(|| {
                            Error::Db(format!("unknown UCUM base dimension '{dim}'"))
                        })?;
                        base_units.insert(code, dv);
                    }
                    b"unit" => {
                        let code = attr(&e, b"Code")?
                            .ok_or_else(|| Error::Db("unit without Code".into()))?;
                        let is_metric = attr(&e, b"isMetric")?.map(|v| v == "yes").unwrap_or(false);
                        let is_special =
                            attr(&e, b"isSpecial")?.map(|v| v == "yes").unwrap_or(false);
                        let is_arbitrary = attr(&e, b"isArbitrary")?
                            .map(|v| v == "yes")
                            .unwrap_or(false);
                        let class = attr(&e, b"class")?;

                        cur_unit = Some(UnitBuilder {
                            code,
                            is_metric,
                            is_special,
                            is_arbitrary,
                            class,
                            value_factor: None,
                            value_unit: None,
                            function: None,
                        });
                    }
                    b"value" => {
                        // Applies to both <prefix> and <unit>.
                        if let Some(p) = cur_prefix.as_mut() {
                            if p.value.is_none() {
                                if let Some(v) = attr(&e, b"value")? {
                                    p.value = Some(parse_rational(&v)?);
                                }
                            }
                        }
                        if let Some(u) = cur_unit.as_mut() {
                            if u.value_unit.is_none() {
                                u.value_unit = attr(&e, b"Unit")?;
                            }
                            if u.value_factor.is_none() {
                                if let Some(v) = attr(&e, b"value")? {
                                    u.value_factor = Some(parse_rational(&v)?);
                                }
                            }
                        }
                    }
                    b"function" => {
                        if let Some(u) = cur_unit.as_mut() {
                            let name = attr(&e, b"name")?
                                .ok_or_else(|| Error::Db("function without name".into()))?;
                            let value = attr(&e, b"value")?
                                .ok_or_else(|| Error::Db("function without value".into()))?;
                            let unit = attr(&e, b"Unit")?
                                .ok_or_else(|| Error::Db("function without Unit".into()))?;
                            u.function = Some((name, parse_rational(&value)?, unit));
                        }
                    }
                    _ => {}
                },
                Event::Empty(e) => {
                    // Treat as Start+End for our purposes.
                    match e.name().as_ref() {
                        b"value" => {
                            if let Some(p) = cur_prefix.as_mut() {
                                if p.value.is_none() {
                                    if let Some(v) = attr(&e, b"value")? {
                                        p.value = Some(parse_rational(&v)?);
                                    }
                                }
                            }
                            if let Some(u) = cur_unit.as_mut() {
                                if u.value_unit.is_none() {
                                    u.value_unit = attr(&e, b"Unit")?;
                                }
                                if u.value_factor.is_none() {
                                    if let Some(v) = attr(&e, b"value")? {
                                        u.value_factor = Some(parse_rational(&v)?);
                                    }
                                }
                            }
                        }
                        b"function" => {
                            if let Some(u) = cur_unit.as_mut() {
                                let name = attr(&e, b"name")?
                                    .ok_or_else(|| Error::Db("function without name".into()))?;
                                let value = attr(&e, b"value")?
                                    .ok_or_else(|| Error::Db("function without value".into()))?;
                                let unit = attr(&e, b"Unit")?
                                    .ok_or_else(|| Error::Db("function without Unit".into()))?;
                                u.function = Some((name, parse_rational(&value)?, unit));
                            }
                        }
                        _ => {}
                    }
                }
                Event::End(e) => match e.name().as_ref() {
                    b"prefix" => {
                        if let Some(p) = cur_prefix.take() {
                            let value = p.value.ok_or_else(|| {
                                Error::Db(format!("prefix '{}' missing value", p.code))
                            })?;
                            prefixes.insert(p.code, value);
                        }
                    }
                    b"unit" => {
                        if let Some(u) = cur_unit.take() {
                            let def = if let Some((name, value, unit)) = u.function {
                                match name.as_str() {
                                    "Cel" | "degF" | "degRe" => {
                                        UnitValueDef::Function { name, value, unit }
                                    }
                                    _ => UnitValueDef::NonLinear { _name: name, _value: value, unit },
                                }
                            } else if let Some(unit_expr) = u.value_unit.clone() {
                                UnitValueDef::Linear {
                                    factor: u.value_factor.unwrap_or_else(BigRational::one),
                                    unit: unit_expr,
                                }
                            } else {
                                UnitValueDef::Base
                            };

                            units.insert(
                                u.code.clone(),
                                UnitDef {
                                    code: u.code,
                                    is_metric: u.is_metric,
                                    is_special: u.is_special,
                                    is_arbitrary: u.is_arbitrary,
                                    class: u.class,
                                    def,
                                },
                            );
                        }
                    }
                    _ => {}
                },
                Event::Eof => break,
                _ => {}
            }
            buf.clear();
        }

        // UCUM base units (UCUM-essence doesn't model mole as base-unit).
        base_units
            .entry("mol".into())
            .or_insert(DimensionVector::AMOUNT);

        Ok(Self {
            prefixes,
            base_units,
            units,
        })
    }
}

fn attr(e: &BytesStart<'_>, key: &[u8]) -> Result<Option<String>> {
    for a in e.attributes() {
        let a = a.map_err(|err| Error::Db(err.to_string()))?;
        if a.key.as_ref() == key {
            return Ok(Some(
                a.unescape_value()
                    .map_err(|err| Error::Db(err.to_string()))?
                    .to_string(),
            ));
        }
    }
    Ok(None)
}

fn parse_rational(s: &str) -> Result<BigRational> {
    // Supports `123`, `123.45`, `1e-3`, `980665e-5`.
    let s = s.trim();
    if s.is_empty() {
        return Err(Error::Db("empty numeric value".into()));
    }

    let mut sign = BigInt::one();
    let mut rest = s;
    if let Some(r) = rest.strip_prefix('+') {
        rest = r;
    } else if let Some(r) = rest.strip_prefix('-') {
        rest = r;
        sign = -sign;
    }

    let (mantissa, exp10) = if let Some((m, e)) = rest.split_once(['e', 'E']) {
        let e: i32 = e
            .parse()
            .map_err(|_| Error::Db(format!("bad exponent '{e}'")))?;
        (m, e)
    } else {
        (rest, 0)
    };

    let (digits, scale) = if let Some((a, b)) = mantissa.split_once('.') {
        (format!("{a}{b}"), b.len() as i32)
    } else {
        (mantissa.to_string(), 0)
    };

    if digits.is_empty() || !digits.as_bytes().iter().all(|c| c.is_ascii_digit()) {
        return Err(Error::Db(format!("bad numeric literal '{s}'")));
    }

    let mut num = BigInt::parse_bytes(digits.as_bytes(), 10)
        .ok_or_else(|| Error::Db(format!("bad integer '{digits}'")))?;
    num *= sign;

    let pow10 = exp10 - scale;
    if pow10 >= 0 {
        let ten = BigInt::from(10u8);
        let mul = ten.pow(pow10 as u32);
        Ok(BigRational::from_integer(num * mul))
    } else {
        let ten = BigInt::from(10u8);
        let den = ten.pow((-pow10) as u32);
        Ok(BigRational::new(num, den))
    }
}

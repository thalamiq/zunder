use crate::ast::{Atom, Term, UnitExpr};
use crate::error::{Error, Result};

pub fn parse(input: &str) -> Result<UnitExpr> {
    if !input.is_ascii() {
        return Err(Error::NonAscii);
    }
    if input.chars().any(|c| c.is_whitespace()) {
        return Err(Error::ContainsWhitespace);
    }
    if input.is_empty() {
        return Err(Error::Syntax {
            pos: 0,
            message: "empty expression",
        });
    }

    let mut parser = Parser::new(input);
    let expr = parser.parse_expr()?;
    if parser.pos != parser.bytes.len() {
        return Err(Error::Syntax {
            pos: parser.pos,
            message: "unexpected trailing input",
        });
    }
    Ok(expr)
}

pub fn validate(input: &str) -> Result<()> {
    crate::unit::Unit::parse(input).map(|_| ())
}

struct Parser<'a> {
    input: &'a str,
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            input,
            bytes: input.as_bytes(),
            pos: 0,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn eat(&mut self, ch: u8) -> bool {
        if self.peek() == Some(ch) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, ch: u8) -> Result<()> {
        if self.eat(ch) {
            Ok(())
        } else {
            Err(Error::Syntax {
                pos: self.pos,
                message: "unexpected character",
            })
        }
    }

    fn parse_expr(&mut self) -> Result<UnitExpr> {
        let mut expr = UnitExpr::one();

        // Leading division is allowed (`/s` means `1/s`).
        if self.eat(b'/') {
            let den = self.parse_product()?;
            self.push_factors(&mut expr, Side::Denominator, den)?;
            while self.eat(b'/') {
                let den_more = self.parse_product()?;
                self.push_factors(&mut expr, Side::Denominator, den_more)?;
            }
            return Ok(expr);
        }

        let num = self.parse_product()?;
        self.push_factors(&mut expr, Side::Numerator, num)?;

        while self.eat(b'/') {
            let den = self.parse_product()?;
            self.push_factors(&mut expr, Side::Denominator, den)?;
        }

        Ok(expr)
    }

    fn parse_product(&mut self) -> Result<Vec<(Term, i32)>> {
        let mut factors = Vec::new();
        let first = self.parse_factor()?;
        factors.push(first);
        while self.eat(b'.') {
            let f = self.parse_factor()?;
            factors.push(f);
        }
        Ok(factors)
    }

    fn parse_factor(&mut self) -> Result<(Term, i32)> {
        let term = self.parse_term()?;
        let exp = self.parse_exponent()?.unwrap_or(1);
        if exp == 0 {
            return Err(Error::Syntax {
                pos: self.pos,
                message: "zero exponent is not allowed",
            });
        }
        Ok((term, exp))
    }

    fn parse_term(&mut self) -> Result<Term> {
        if self.eat(b'(') {
            let expr = self.parse_expr()?;
            self.expect(b')')?;
            Ok(Term::Group(Box::new(expr)))
        } else {
            Ok(Term::Atom(self.parse_atom()?))
        }
    }

    fn parse_exponent(&mut self) -> Result<Option<i32>> {
        let start = self.pos;
        let mut sign: i32 = 1;
        let Some(b) = self.peek() else {
            return Ok(None);
        };
        match b {
            b'+' => {
                self.pos += 1;
            }
            b'-' => {
                sign = -1;
                self.pos += 1;
            }
            b'0'..=b'9' => {}
            _ => return Ok(None),
        }

        let mut value: i32 = 0;
        let mut saw_digit = false;
        while let Some(b'0'..=b'9') = self.peek() {
            saw_digit = true;
            let digit = (self.bytes[self.pos] - b'0') as i32;
            value = value
                .checked_mul(10)
                .and_then(|v| v.checked_add(digit))
                .ok_or(Error::Overflow)?;
            self.pos += 1;
        }

        if !saw_digit {
            self.pos = start;
            return Ok(None);
        }

        value.checked_mul(sign).ok_or(Error::Overflow).map(Some)
    }

    fn parse_atom(&mut self) -> Result<Atom> {
        let b = self.peek().ok_or(Error::Syntax {
            pos: self.pos,
            message: "unexpected end of input",
        })?;

        if b.is_ascii_digit() {
            // Either an integer scalar (e.g. `/12`) or `10*`/`10^` family.
            let start = self.pos;
            while let Some(c) = self.peek() {
                if !c.is_ascii_digit() {
                    break;
                }
                self.pos += 1;
            }
            let digits = &self.input[start..self.pos];
            match self.peek() {
                Some(b'*') | Some(b'^') => {
                    let op = self.bytes[self.pos] as char;
                    self.pos += 1;
                    return Ok(Atom::Symbol(format!("{digits}{op}")));
                }
                Some(next) if next.is_ascii_alphabetic() || next == b'[' || next == b'_' => {
                    return Err(Error::Syntax {
                        pos: self.pos,
                        message: "unexpected letter after integer",
                    });
                }
                _ => {
                    let value: u64 = digits.parse().map_err(|_| Error::Overflow)?;
                    return Ok(Atom::Integer(value));
                }
            }
        }

        let symbol = self.parse_symbol()?;
        Ok(Atom::Symbol(symbol))
    }

    fn parse_symbol(&mut self) -> Result<String> {
        let mut out = String::new();

        while let Some(b) = self.peek() {
            match b {
                b'(' | b')' | b'.' | b'/' => break,
                b'0'..=b'9' => break, // exponent begins
                b'+' | b'-' => break, // exponent begins
                b'[' => out.push_str(&self.parse_bracket_segment()?),
                b'a'..=b'z' | b'A'..=b'Z' => {
                    out.push(b as char);
                    self.pos += 1;
                }
                b'%' | b'_' | b'\'' => {
                    out.push(b as char);
                    self.pos += 1;
                }
                b'*' | b'^' => {
                    out.push(b as char);
                    self.pos += 1;
                }
                _ => {
                    return Err(Error::Syntax {
                        pos: self.pos,
                        message: "invalid character in unit symbol",
                    });
                }
            }
        }

        if out.is_empty() {
            Err(Error::Syntax {
                pos: self.pos,
                message: "expected unit symbol",
            })
        } else {
            Ok(out)
        }
    }

    fn parse_bracket_segment(&mut self) -> Result<String> {
        let start = self.pos;
        self.expect(b'[')?;
        while let Some(b) = self.peek() {
            self.pos += 1;
            if b == b']' {
                return Ok(self.input[start..self.pos].to_string());
            }
        }
        Err(Error::Syntax {
            pos: start,
            message: "unclosed bracketed unit",
        })
    }

    fn push_factors(
        &self,
        expr: &mut UnitExpr,
        side: Side,
        factors: Vec<(Term, i32)>,
    ) -> Result<()> {
        for (term, exp) in factors {
            self.push_factor(expr, side, term, exp)?;
        }
        Ok(())
    }

    fn push_factor(&self, expr: &mut UnitExpr, side: Side, term: Term, exp: i32) -> Result<()> {
        if exp == 0 {
            return Err(Error::Overflow);
        }
        let (target, e) = if exp > 0 {
            (side, exp)
        } else {
            (side.flip(), -exp)
        };
        match target {
            Side::Numerator => expr.numerator.push((term, e)),
            Side::Denominator => expr.denominator.push((term, e)),
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug)]
enum Side {
    Numerator,
    Denominator,
}

impl Side {
    fn flip(self) -> Self {
        match self {
            Side::Numerator => Side::Denominator,
            Side::Denominator => Side::Numerator,
        }
    }
}

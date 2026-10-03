//! Werkzeug: Rechner mit exakter Ganzzahl- und rationaler Arithmetik.
//!
//! Der MVP nutzt ein handgeschriebenes Shunting-Yard-Parser über einen kleinen
//! rationalen Datentyp (Zähler i128, Nenner i128). Das reicht für die im
//! Konzept 8.3 gelisteten Kernfälle (Rechnen im Chat, Prozente, gebrochene
//! Zahlen) ohne extra Crate-Abhängigkeit.

use pa_policy::{CapabilityAction, CapabilityRequest, DerivationSource};
use serde_json::json;

use crate::{
    evaluate_and_audit, require_string, Tool, ToolContext, ToolError, ToolInvocation, ToolOutput,
    ToolSpec,
};

pub struct CalculatorTool;

impl Tool for CalculatorTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "calculator".to_owned(),
            description:
                "Rechnet mit exakten Brüchen (+, -, *, /, %, ^, Klammern). Rundungen nur bei sehr großen Ergebnissen."
                    .to_owned(),
            parameters_schema: json!({
                "type": "object",
                "required": ["expression"],
                "properties": { "expression": { "type": "string" } }
            }),
            category: "analysis".to_owned(),
        }
    }

    fn invoke(
        &self,
        invocation: &ToolInvocation,
        context: &mut ToolContext<'_>,
    ) -> Result<ToolOutput, ToolError> {
        let expression = require_string(invocation, "expression")?;
        let request = CapabilityRequest {
            action: CapabilityAction::Pure,
            relative_path: None,
            source: invocation.source,
            reason: format!("calculator `{expression}`"),
        };
        let _capability = evaluate_and_audit(&invocation.name, &request, context)?;
        let value = evaluate_expression(&expression).map_err(|reason| ToolError::Invalid {
            tool: invocation.name.clone(),
            reason,
        })?;
        Ok(ToolOutput {
            tool: invocation.name.clone(),
            content: format_rational(value),
            is_untrusted: matches!(invocation.source, DerivationSource::UntrustedContent),
            truncated_from_bytes: None,
        })
    }
}

// ------------------- rationaler Kern --------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Rat {
    num: i128,
    den: i128,
}

impl Rat {
    fn new(num: i128, den: i128) -> Result<Self, String> {
        if den == 0 {
            return Err("Division durch Null".to_owned());
        }
        let (num, den) = if den < 0 { (-num, -den) } else { (num, den) };
        let g = gcd(num.abs(), den);
        Ok(Self {
            num: num / g,
            den: den / g,
        })
    }

    fn from_int(n: i128) -> Self {
        Self { num: n, den: 1 }
    }

    fn add(self, other: Rat) -> Result<Rat, String> {
        Rat::new(
            self.num
                .checked_mul(other.den)
                .ok_or("Überlauf")?
                .checked_add(other.num.checked_mul(self.den).ok_or("Überlauf")?)
                .ok_or("Überlauf")?,
            self.den.checked_mul(other.den).ok_or("Überlauf")?,
        )
    }

    fn sub(self, other: Rat) -> Result<Rat, String> {
        self.add(Rat {
            num: -other.num,
            den: other.den,
        })
    }

    fn mul(self, other: Rat) -> Result<Rat, String> {
        Rat::new(
            self.num.checked_mul(other.num).ok_or("Überlauf")?,
            self.den.checked_mul(other.den).ok_or("Überlauf")?,
        )
    }

    fn div(self, other: Rat) -> Result<Rat, String> {
        if other.num == 0 {
            return Err("Division durch Null".to_owned());
        }
        Rat::new(
            self.num.checked_mul(other.den).ok_or("Überlauf")?,
            self.den.checked_mul(other.num).ok_or("Überlauf")?,
        )
    }

    fn pow_i(self, exponent: i128) -> Result<Rat, String> {
        if exponent < 0 {
            let base = self.pow_i(-exponent)?;
            return Rat::from_int(1).div(base);
        }
        let mut result = Rat::from_int(1);
        let mut base = self;
        let mut n = exponent as u128;
        while n > 0 {
            if n & 1 == 1 {
                result = result.mul(base)?;
            }
            n >>= 1;
            if n > 0 {
                base = base.mul(base)?;
            }
        }
        Ok(result)
    }
}

fn gcd(a: i128, b: i128) -> i128 {
    let (mut a, mut b) = (a.abs(), b.abs());
    while b != 0 {
        let temp = b;
        b = a % b;
        a = temp;
    }
    if a == 0 {
        1
    } else {
        a
    }
}

fn format_rational(value: Rat) -> String {
    if value.den == 1 {
        value.num.to_string()
    } else {
        format!(
            "{}/{} ≈ {:.6}",
            value.num,
            value.den,
            value.num as f64 / value.den as f64
        )
    }
}

// ------------------- Parser -----------------------------

fn evaluate_expression(input: &str) -> Result<Rat, String> {
    let tokens = tokenize(input)?;
    let (value, rest) = parse_expr(&tokens, 0)?;
    if !rest.is_empty() {
        return Err(format!("unerwartete Reste: {:?}", rest));
    }
    Ok(value)
}

#[derive(Debug, Clone)]
enum Token {
    Number(i128),
    Op(char),
    LParen,
    RParen,
}

fn tokenize(input: &str) -> Result<Vec<Token>, String> {
    let mut result = Vec::new();
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if c.is_ascii_digit() {
            let start = i;
            while i < bytes.len() && (bytes[i] as char).is_ascii_digit() {
                i += 1;
            }
            let n: i128 = input[start..i]
                .parse()
                .map_err(|_| format!("ungültige Zahl bei Position {start}"))?;
            result.push(Token::Number(n));
        } else if "+-*/%^".contains(c) {
            result.push(Token::Op(c));
            i += 1;
        } else if c == '(' {
            result.push(Token::LParen);
            i += 1;
        } else if c == ')' {
            result.push(Token::RParen);
            i += 1;
        } else {
            return Err(format!("unerlaubtes Zeichen `{c}` bei Position {i}"));
        }
    }
    Ok(result)
}

fn precedence(op: char) -> u8 {
    match op {
        '+' | '-' => 1,
        '*' | '/' | '%' => 2,
        '^' => 3,
        _ => 0,
    }
}

fn parse_expr(tokens: &[Token], min_prec: u8) -> Result<(Rat, &[Token]), String> {
    let (mut lhs, mut rest) = parse_atom(tokens)?;
    while let Some(Token::Op(op)) = rest.first() {
        let prec = precedence(*op);
        if prec < min_prec {
            break;
        }
        let next_min = if *op == '^' { prec } else { prec + 1 };
        let (rhs, next_rest) = parse_expr(&rest[1..], next_min)?;
        lhs = match op {
            '+' => lhs.add(rhs)?,
            '-' => lhs.sub(rhs)?,
            '*' => lhs.mul(rhs)?,
            '/' => lhs.div(rhs)?,
            '%' => {
                let hundred = Rat::from_int(100);
                lhs.mul(rhs)?.div(hundred)?
            }
            '^' => {
                if rhs.den != 1 {
                    return Err("Exponent muss ganzzahlig sein".to_owned());
                }
                lhs.pow_i(rhs.num)?
            }
            other => return Err(format!("unbekannter Operator `{other}`")),
        };
        rest = next_rest;
    }
    Ok((lhs, rest))
}

fn parse_atom(tokens: &[Token]) -> Result<(Rat, &[Token]), String> {
    match tokens.first() {
        Some(Token::Number(n)) => Ok((Rat::from_int(*n), &tokens[1..])),
        Some(Token::Op('-')) => {
            let (value, rest) = parse_atom(&tokens[1..])?;
            Ok((Rat::from_int(0).sub(value)?, rest))
        }
        Some(Token::LParen) => {
            let (value, rest) = parse_expr(&tokens[1..], 0)?;
            match rest.first() {
                Some(Token::RParen) => Ok((value, &rest[1..])),
                _ => Err("fehlende schließende Klammer".to_owned()),
            }
        }
        Some(other) => Err(format!("unerwartetes Token: {other:?}")),
        None => Err("leere Eingabe".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_arithmetic_matches_exact_result() {
        assert_eq!(evaluate_expression("2+3*4").unwrap(), Rat::from_int(14));
        assert_eq!(evaluate_expression("(2+3)*4").unwrap(), Rat::from_int(20));
        assert_eq!(
            evaluate_expression("1/3+1/3+1/3").unwrap(),
            Rat::from_int(1)
        );
        assert_eq!(evaluate_expression("2^10").unwrap(), Rat::from_int(1024));
    }

    #[test]
    fn division_by_zero_reports_error() {
        let err = evaluate_expression("1/0").unwrap_err();
        assert!(err.contains("Null"));
    }

    #[test]
    fn percent_operator_is_evaluated() {
        // 15 % Zinsen auf 200: (15*200)/100 = 30
        assert_eq!(evaluate_expression("15%200").unwrap(), Rat::from_int(30));
    }
}

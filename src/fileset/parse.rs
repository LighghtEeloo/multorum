//! Recursive descent parser for file set expressions.
//!
//! ## Grammar
//!
//! ```text
//! expr  ::= atom (op atom)*
//! atom  ::= name | "(" expr ")"
//! op    ::= "|" | "&" | "-"
//! name  ::= [A-Z][A-Za-z0-9]*
//! ```
//!
//! All binary operators have equal precedence with left-to-right
//! associativity. Use parentheses to change grouping.

use super::error::ParseError;
use super::expr::Expr;
use super::name::Name;

/// A binary set operator.
#[derive(Debug, Clone, Copy)]
enum Op {
    Union,
    Intersection,
    Difference,
}

/// Recursive descent parser for file set expressions.
///
/// Constructed from an input string and consumed by [`ExprParser::parse`],
/// which returns the parsed [`Expr`] or a [`ParseError`] with position
/// information.
pub struct ExprParser<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> ExprParser<'a> {
    /// Create a parser for the given expression string.
    pub fn new(input: &'a str) -> Self {
        Self { input, pos: 0 }
    }

    /// Parse the complete input into an expression.
    ///
    /// Fails if the input is empty or has trailing unparsed content.
    pub fn parse(mut self) -> Result<Expr, ParseError> {
        self.skip_whitespace();
        if self.is_eof() {
            return Err(ParseError::UnexpectedEof);
        }
        let expr = self.parse_expr()?;
        self.skip_whitespace();
        if !self.is_eof() {
            return Err(ParseError::TrailingContent { pos: self.pos });
        }
        Ok(expr)
    }

    /// Parse `expr ::= atom (op atom)*` with left-to-right associativity.
    fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_atom()?;
        while let Some(op) = self.try_parse_op() {
            let right = self.parse_atom()?;
            left = match op {
                | Op::Union => Expr::Union(Box::new(left), Box::new(right)),
                | Op::Intersection => Expr::Intersection(Box::new(left), Box::new(right)),
                | Op::Difference => Expr::Difference(Box::new(left), Box::new(right)),
            };
        }
        Ok(left)
    }

    /// Parse `atom ::= name | "(" expr ")"`.
    fn parse_atom(&mut self) -> Result<Expr, ParseError> {
        self.skip_whitespace();
        match self.peek() {
            | None => Err(ParseError::UnexpectedEof),
            | Some('(') => {
                self.advance();
                let expr = self.parse_expr()?;
                self.skip_whitespace();
                match self.peek() {
                    | Some(')') => {
                        self.advance();
                        Ok(expr)
                    }
                    | _ => Err(ParseError::UnclosedParen { pos: self.pos }),
                }
            }
            | Some(ch) if ch.is_ascii_uppercase() => {
                let name = self.parse_name()?;
                Ok(Expr::Ref(name))
            }
            | Some(ch) => Err(ParseError::UnexpectedChar { ch, pos: self.pos }),
        }
    }

    /// Parse `name ::= [A-Z][A-Za-z0-9]*`.
    fn parse_name(&mut self) -> Result<Name, ParseError> {
        let start = self.pos;
        while let Some(ch) = self.peek() {
            if ch.is_ascii_alphanumeric() {
                self.advance();
            } else {
                break;
            }
        }
        let raw = &self.input[start..self.pos];
        Name::new(raw).map_err(ParseError::InvalidName)
    }

    /// Try to consume an operator, returning `None` if the next
    /// non-whitespace character is not an operator.
    fn try_parse_op(&mut self) -> Option<Op> {
        self.skip_whitespace();
        match self.peek()? {
            | '|' => {
                self.advance();
                Some(Op::Union)
            }
            | '&' => {
                self.advance();
                Some(Op::Intersection)
            }
            | '-' => {
                self.advance();
                Some(Op::Difference)
            }
            | _ => None,
        }
    }

    fn skip_whitespace(&mut self) {
        while let Some(ch) = self.peek() {
            if ch.is_ascii_whitespace() {
                self.advance();
            } else {
                break;
            }
        }
    }

    fn peek(&self) -> Option<char> {
        self.input[self.pos..].chars().next()
    }

    fn advance(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.pos += ch.len_utf8();
        Some(ch)
    }

    fn is_eof(&self) -> bool {
        self.pos >= self.input.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Result<Expr, ParseError> {
        ExprParser::new(s).parse()
    }

    fn name(s: &str) -> Expr {
        Expr::Ref(Name::new(s).unwrap())
    }

    #[test]
    fn single_name() {
        assert_eq!(parse("AuthFiles").unwrap(), name("AuthFiles"));
    }

    #[test]
    fn union() {
        let expected = Expr::Union(Box::new(name("A")), Box::new(name("B")));
        assert_eq!(parse("A | B").unwrap(), expected);
    }

    #[test]
    fn intersection() {
        let expected = Expr::Intersection(Box::new(name("A")), Box::new(name("B")));
        assert_eq!(parse("A & B").unwrap(), expected);
    }

    #[test]
    fn difference() {
        let expected = Expr::Difference(Box::new(name("A")), Box::new(name("B")));
        assert_eq!(parse("A - B").unwrap(), expected);
    }

    #[test]
    fn left_associativity() {
        // A - B - C  →  (A - B) - C
        let ab = Expr::Difference(Box::new(name("A")), Box::new(name("B")));
        let expected = Expr::Difference(Box::new(ab), Box::new(name("C")));
        assert_eq!(parse("A - B - C").unwrap(), expected);
    }

    #[test]
    fn flat_precedence() {
        // A | B & C  →  (A | B) & C  (not A | (B & C))
        let a_or_b = Expr::Union(Box::new(name("A")), Box::new(name("B")));
        let expected = Expr::Intersection(Box::new(a_or_b), Box::new(name("C")));
        assert_eq!(parse("A | B & C").unwrap(), expected);
    }

    #[test]
    fn parenthesized_grouping() {
        // A | (B & C)
        let b_and_c = Expr::Intersection(Box::new(name("B")), Box::new(name("C")));
        let expected = Expr::Union(Box::new(name("A")), Box::new(b_and_c));
        assert_eq!(parse("A | (B & C)").unwrap(), expected);
    }

    #[test]
    fn nested_parens() {
        // ((A | B))
        let inner = Expr::Union(Box::new(name("A")), Box::new(name("B")));
        assert_eq!(parse("((A | B))").unwrap(), inner);
    }

    #[test]
    fn whitespace_is_flexible() {
        let expected = Expr::Union(Box::new(name("A")), Box::new(name("B")));
        assert_eq!(parse("  A  |  B  ").unwrap(), expected);
        assert_eq!(parse("A|B").unwrap(), expected);
    }

    #[test]
    fn empty_input_is_error() {
        assert!(matches!(parse(""), Err(ParseError::UnexpectedEof)));
        assert!(matches!(parse("   "), Err(ParseError::UnexpectedEof)));
    }

    #[test]
    fn unclosed_paren() {
        assert!(matches!(parse("(A | B"), Err(ParseError::UnclosedParen { .. })));
    }

    #[test]
    fn trailing_content() {
        assert!(matches!(parse("A B"), Err(ParseError::TrailingContent { .. })));
    }

    #[test]
    fn unexpected_char() {
        assert!(matches!(parse("A | !B"), Err(ParseError::UnexpectedChar { ch: '!', .. })));
    }

    #[test]
    fn design_doc_example() {
        // "AuthFiles - AuthSpecs - AuthTests"  →  (AuthFiles - AuthSpecs) - AuthTests
        let step1 = Expr::Difference(Box::new(name("AuthFiles")), Box::new(name("AuthSpecs")));
        let expected = Expr::Difference(Box::new(step1), Box::new(name("AuthTests")));
        assert_eq!(parse("AuthFiles - AuthSpecs - AuthTests").unwrap(), expected);
    }
}

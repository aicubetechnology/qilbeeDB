//! Simple Cypher Parser
//!
//! Basic recursive descent parser for common Cypher patterns.
//! Supports: MATCH, WHERE, RETURN, ORDER BY, LIMIT

use crate::parser::*;
use qilbee_core::{Error, Result};

/// Parse a Cypher query string
pub fn parse_simple(query: &str) -> Result<Query> {
    let mut parser = SimpleParser::new(query);
    parser.parse_query()
}

struct SimpleParser {
    query: String,
    pos: usize,
}

impl SimpleParser {
    fn new(query: &str) -> Self {
        Self {
            query: query.trim().to_string(),
            pos: 0,
        }
    }

    fn parse_query(&mut self) -> Result<Query> {
        let mut clauses = Vec::new();

        // Parse MATCH clause
        if self.consume_keyword("MATCH") {
            let match_clause = self.parse_match()?;
            clauses.push(Clause::Match(match_clause));
        }

        // Parse WHERE clause
        if self.consume_keyword("WHERE") {
            let where_expr = self.parse_where()?;
            clauses.push(Clause::Where(where_expr));
        }

        // Parse RETURN clause
        if self.consume_keyword("RETURN") {
            let return_clause = self.parse_return()?;
            clauses.push(Clause::Return(return_clause));
        }

        // Parse ORDER BY clause
        if self.consume_keyword("ORDER") {
            self.consume_keyword("BY");
            let order_clause = self.parse_order_by()?;
            clauses.push(Clause::OrderBy(order_clause));
        }

        // Parse LIMIT clause
        if self.consume_keyword("LIMIT") {
            let limit_expr = self.parse_limit()?;
            clauses.push(Clause::Limit(limit_expr));
        }

        Ok(Query { clauses })
    }

    fn parse_match(&mut self) -> Result<MatchClause> {
        self.skip_whitespace();

        // Parse pattern: (variable:Label)
        if !self.consume_char('(') {
            return Err(Error::QueryParse("Expected '(' after MATCH".to_string()));
        }

        let variable = self.parse_identifier()?;

        let mut labels = Vec::new();
        if self.consume_char(':') {
            labels.push(self.parse_identifier()?);
        }

        if !self.consume_char(')') {
            return Err(Error::QueryParse("Expected ')' in pattern".to_string()));
        }

        let node_pattern = NodePattern {
            variable: Some(variable),
            labels,
            properties: None,
        };

        Ok(MatchClause {
            patterns: vec![Pattern {
                elements: vec![PatternElement::Node(node_pattern)],
            }],
        })
    }

    fn parse_where(&mut self) -> Result<Expression> {
        self.skip_whitespace();

        // Parse comparison: variable.property > $parameter
        let left = self.parse_expression()?;

        self.skip_whitespace();
        let op = self.parse_operator()?;

        self.skip_whitespace();
        let right = self.parse_expression()?;

        Ok(Expression::Binary {
            left: Box::new(left),
            op,
            right: Box::new(right),
        })
    }

    fn parse_return(&mut self) -> Result<ReturnClause> {
        self.skip_whitespace();

        let mut items = Vec::new();

        loop {
            let expr = self.parse_expression()?;
            let alias = None; // TODO: parse AS alias

            items.push(ReturnItem {
                expression: expr,
                alias,
            });

            self.skip_whitespace();
            if !self.consume_char(',') {
                break;
            }
        }

        Ok(ReturnClause {
            distinct: false,
            items,
        })
    }

    fn parse_order_by(&mut self) -> Result<OrderByClause> {
        self.skip_whitespace();

        let expr = self.parse_expression()?;

        self.skip_whitespace();
        let descending = self.consume_keyword("DESC");

        Ok(OrderByClause {
            items: vec![OrderItem {
                expression: expr,
                ascending: !descending,
            }],
        })
    }

    fn parse_limit(&mut self) -> Result<Expression> {
        self.skip_whitespace();
        let num = self.parse_number()?;
        Ok(Expression::Literal(Literal::Integer(num)))
    }

    fn parse_expression(&mut self) -> Result<Expression> {
        self.skip_whitespace();

        // Check for parameter
        if self.consume_char('$') {
            let param_name = self.parse_identifier()?;
            return Ok(Expression::Parameter(param_name));
        }

        // Check for number
        if self.peek_char().is_some_and(|c| c.is_ascii_digit()) {
            let num = self.parse_number()?;
            return Ok(Expression::Literal(Literal::Integer(num)));
        }

        // Check for string
        if self.peek_char() == Some('\'') || self.peek_char() == Some('"') {
            let s = self.parse_string()?;
            return Ok(Expression::Literal(Literal::String(s)));
        }

        // Parse variable or property access
        let var = self.parse_identifier()?;

        // Check for property access
        if self.consume_char('.') {
            let prop = self.parse_identifier()?;
            return Ok(Expression::Property(
                Box::new(Expression::Variable(var)),
                prop,
            ));
        }

        Ok(Expression::Variable(var))
    }

    fn parse_operator(&mut self) -> Result<BinaryOp> {
        self.skip_whitespace();

        if self.consume_str(">=") {
            Ok(BinaryOp::GreaterEquals)
        } else if self.consume_str("<=") {
            Ok(BinaryOp::LessEquals)
        } else if self.consume_str("!=") || self.consume_str("<>") {
            Ok(BinaryOp::NotEquals)
        } else if self.consume_char('>') {
            Ok(BinaryOp::GreaterThan)
        } else if self.consume_char('<') {
            Ok(BinaryOp::LessThan)
        } else if self.consume_char('=') {
            Ok(BinaryOp::Equals)
        } else {
            Err(Error::QueryParse("Expected comparison operator".to_string()))
        }
    }

    fn parse_identifier(&mut self) -> Result<String> {
        self.skip_whitespace();

        let start = self.pos;
        while let Some(c) = self.peek_char() {
            if c.is_alphanumeric() || c == '_' {
                self.pos += 1;
            } else {
                break;
            }
        }

        if start == self.pos {
            return Err(Error::QueryParse("Expected identifier".to_string()));
        }

        Ok(self.query[start..self.pos].to_string())
    }

    fn parse_number(&mut self) -> Result<i64> {
        self.skip_whitespace();

        let start = self.pos;
        while let Some(c) = self.peek_char() {
            if c.is_ascii_digit() {
                self.pos += 1;
            } else {
                break;
            }
        }

        if start == self.pos {
            return Err(Error::QueryParse("Expected number".to_string()));
        }

        self.query[start..self.pos]
            .parse()
            .map_err(|_| Error::QueryParse("Invalid number".to_string()))
    }

    fn parse_string(&mut self) -> Result<String> {
        self.skip_whitespace();

        let quote = self.consume_any_char()
            .ok_or_else(|| Error::QueryParse("Expected quote".to_string()))?;

        if quote != '\'' && quote != '"' {
            return Err(Error::QueryParse("Expected string".to_string()));
        }

        let start = self.pos;
        while let Some(c) = self.peek_char() {
            if c == quote {
                break;
            }
            self.pos += 1;
        }

        let s = self.query[start..self.pos].to_string();

        if !self.consume_char(quote) {
            return Err(Error::QueryParse("Unterminated string".to_string()));
        }

        Ok(s)
    }

    fn consume_keyword(&mut self, keyword: &str) -> bool {
        self.skip_whitespace();

        let upper_keyword = keyword.to_uppercase();
        let remaining = &self.query[self.pos..];

        if remaining.len() >= upper_keyword.len() {
            let candidate = &remaining[..upper_keyword.len()];
            if candidate.to_uppercase() == upper_keyword {
                // Check that it's followed by whitespace or end
                if remaining.len() == upper_keyword.len() ||
                   remaining.chars().nth(upper_keyword.len()).map_or(false, |c| c.is_whitespace() || "(),".contains(c)) {
                    self.pos += upper_keyword.len();
                    return true;
                }
            }
        }

        false
    }

    fn consume_str(&mut self, s: &str) -> bool {
        let remaining = &self.query[self.pos..];
        if remaining.starts_with(s) {
            self.pos += s.len();
            true
        } else {
            false
        }
    }

    fn consume_char(&mut self, c: char) -> bool {
        self.skip_whitespace();
        if self.peek_char() == Some(c) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn consume_any_char(&mut self) -> Option<char> {
        if self.pos < self.query.len() {
            let c = self.query.chars().nth(self.pos)?;
            self.pos += 1;
            Some(c)
        } else {
            None
        }
    }

    fn peek_char(&self) -> Option<char> {
        self.query.chars().nth(self.pos)
    }

    fn skip_whitespace(&mut self) {
        while let Some(c) = self.peek_char() {
            if c.is_whitespace() {
                self.pos += 1;
            } else {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_match() {
        let query = "MATCH (p:Person) RETURN p.name";
        let result = parse_simple(query);
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_match_where_return() {
        let query = "MATCH (p:Person) WHERE p.age > $age RETURN p.name, p.age";
        let result = parse_simple(query);
        assert!(result.is_ok());
    }
}

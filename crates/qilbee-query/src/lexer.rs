//! Cypher lexer using logos

use logos::Logos;

/// Cypher tokens
#[derive(Logos, Debug, Clone, PartialEq)]
#[logos(skip r"[ \t\r\n\f]+")]
pub enum Token {
    // Keywords
    #[token("MATCH", ignore(ascii_case))]
    Match,

    #[token("OPTIONAL", ignore(ascii_case))]
    Optional,

    #[token("WHERE", ignore(ascii_case))]
    Where,

    #[token("RETURN", ignore(ascii_case))]
    Return,

    #[token("CREATE", ignore(ascii_case))]
    Create,

    #[token("DELETE", ignore(ascii_case))]
    Delete,

    #[token("DETACH", ignore(ascii_case))]
    Detach,

    #[token("SET", ignore(ascii_case))]
    Set,

    #[token("REMOVE", ignore(ascii_case))]
    Remove,

    #[token("MERGE", ignore(ascii_case))]
    Merge,

    #[token("WITH", ignore(ascii_case))]
    With,

    #[token("UNWIND", ignore(ascii_case))]
    Unwind,

    #[token("ORDER", ignore(ascii_case))]
    Order,

    #[token("BY", ignore(ascii_case))]
    By,

    #[token("SKIP", ignore(ascii_case))]
    Skip,

    #[token("LIMIT", ignore(ascii_case))]
    Limit,

    #[token("ASC", ignore(ascii_case))]
    Asc,

    #[token("DESC", ignore(ascii_case))]
    Desc,

    #[token("AS", ignore(ascii_case))]
    As,

    #[token("DISTINCT", ignore(ascii_case))]
    Distinct,

    #[token("UNION", ignore(ascii_case))]
    Union,

    #[token("ALL", ignore(ascii_case))]
    All,

    #[token("CALL", ignore(ascii_case))]
    Call,

    #[token("YIELD", ignore(ascii_case))]
    Yield,

    #[token("FOREACH", ignore(ascii_case))]
    Foreach,

    #[token("IN", ignore(ascii_case))]
    In,

    #[token("ON", ignore(ascii_case))]
    On,

    #[token("CASE", ignore(ascii_case))]
    Case,

    #[token("WHEN", ignore(ascii_case))]
    When,

    #[token("THEN", ignore(ascii_case))]
    Then,

    #[token("ELSE", ignore(ascii_case))]
    Else,

    #[token("END", ignore(ascii_case))]
    End,

    // Boolean keywords
    #[token("AND", ignore(ascii_case))]
    And,

    #[token("OR", ignore(ascii_case))]
    Or,

    #[token("XOR", ignore(ascii_case))]
    Xor,

    #[token("NOT", ignore(ascii_case))]
    Not,

    #[token("TRUE", ignore(ascii_case))]
    True,

    #[token("FALSE", ignore(ascii_case))]
    False,

    #[token("NULL", ignore(ascii_case))]
    Null,

    #[token("IS", ignore(ascii_case))]
    Is,

    #[token("CONTAINS", ignore(ascii_case))]
    Contains,

    #[token("STARTS", ignore(ascii_case))]
    Starts,

    #[token("ENDS", ignore(ascii_case))]
    Ends,

    // Symbols
    #[token("(")]
    LParen,

    #[token(")")]
    RParen,

    #[token("[")]
    LBracket,

    #[token("]")]
    RBracket,

    #[token("{")]
    LBrace,

    #[token("}")]
    RBrace,

    #[token(":")]
    Colon,

    #[token(",")]
    Comma,

    #[token(".")]
    Dot,

    #[token("|")]
    Pipe,

    #[token("..")]
    DoubleDot,

    #[token("=")]
    Equals,

    #[token("<>")]
    NotEquals,

    #[token("!=")]
    NotEquals2,

    #[token("<")]
    LessThan,

    #[token("<=")]
    LessEquals,

    #[token(">")]
    GreaterThan,

    #[token(">=")]
    GreaterEquals,

    #[token("+")]
    Plus,

    #[token("-")]
    Minus,

    #[token("*")]
    Star,

    #[token("/")]
    Slash,

    #[token("%")]
    Percent,

    #[token("^")]
    Caret,

    #[token("+=")]
    PlusEquals,

    // Relationship arrows
    #[token("-->")]
    ArrowRight,

    #[token("<--")]
    ArrowLeft,

    #[token("--")]
    DoubleDash,

    #[token("->")]
    DashArrowRight,

    #[token("<-")]
    ArrowLeftDash,

    // Literals
    #[regex(r"[0-9]+", |lex| lex.slice().parse::<i64>().ok())]
    Integer(i64),

    #[regex(r"[0-9]+\.[0-9]+([eE][+-]?[0-9]+)?", |lex| lex.slice().parse::<f64>().ok())]
    Float(f64),

    #[regex(r#""([^"\\]|\\.)*""#, |lex| {
        let s = lex.slice();
        Some(s[1..s.len()-1].to_string())
    })]
    StringDouble(String),

    #[regex(r#"'([^'\\]|\\.)*'"#, |lex| {
        let s = lex.slice();
        Some(s[1..s.len()-1].to_string())
    })]
    StringSingle(String),

    // Identifiers
    #[regex(r"[a-zA-Z_][a-zA-Z0-9_]*", |lex| lex.slice().to_string())]
    Identifier(String),

    #[regex(r"`[^`]+`", |lex| {
        let s = lex.slice();
        s[1..s.len()-1].to_string()
    })]
    EscapedIdentifier(String),

    // Parameter
    #[regex(r"\$[a-zA-Z_][a-zA-Z0-9_]*", |lex| lex.slice()[1..].to_string())]
    Parameter(String),

    // Comment (skip)
    #[regex(r"//[^\n]*", logos::skip)]
    LineComment,

    #[regex(r"/\*([^*]|\*[^/])*\*/", logos::skip)]
    BlockComment,
}

impl Token {
    /// Check if this token is a keyword
    pub fn is_keyword(&self) -> bool {
        matches!(
            self,
            Token::Match
                | Token::Optional
                | Token::Where
                | Token::Return
                | Token::Create
                | Token::Delete
                | Token::Set
                | Token::Remove
                | Token::Merge
                | Token::With
                | Token::Unwind
                | Token::Order
                | Token::By
                | Token::Skip
                | Token::Limit
                | Token::And
                | Token::Or
                | Token::Not
                | Token::True
                | Token::False
                | Token::Null
        )
    }

    /// Check if this token is a literal
    pub fn is_literal(&self) -> bool {
        matches!(
            self,
            Token::Integer(_)
                | Token::Float(_)
                | Token::StringDouble(_)
                | Token::StringSingle(_)
                | Token::True
                | Token::False
                | Token::Null
        )
    }
}

/// Tokenize a Cypher query string
pub fn tokenize(input: &str) -> Vec<Token> {
    Token::lexer(input).filter_map(|r| r.ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_query() {
        let tokens = tokenize("MATCH (n:Person) RETURN n");

        assert!(tokens.contains(&Token::Match));
        assert!(tokens.contains(&Token::Return));
        assert!(tokens.contains(&Token::LParen));
        assert!(tokens.contains(&Token::RParen));
        assert!(tokens.contains(&Token::Colon));
    }

    #[test]
    fn test_identifiers() {
        let tokens = tokenize("MATCH (person:Person)");

        assert!(tokens
            .iter()
            .any(|t| matches!(t, Token::Identifier(s) if s == "person")));
        assert!(tokens
            .iter()
            .any(|t| matches!(t, Token::Identifier(s) if s == "Person")));
    }

    #[test]
    fn test_literals() {
        let tokens = tokenize("WHERE n.age = 30 AND n.height = 1.75");

        assert!(tokens.iter().any(|t| matches!(t, Token::Integer(30))));
        assert!(tokens.iter().any(|t| matches!(t, Token::Float(f) if (*f - 1.75).abs() < 0.001)));
    }

    #[test]
    fn test_strings() {
        let tokens = tokenize(r#"WHERE n.name = "Alice" OR n.name = 'Bob'"#);

        assert!(tokens
            .iter()
            .any(|t| matches!(t, Token::StringDouble(s) if s == "Alice")));
        assert!(tokens
            .iter()
            .any(|t| matches!(t, Token::StringSingle(s) if s == "Bob")));
    }

    #[test]
    fn test_relationship_pattern() {
        let tokens = tokenize("MATCH (a)-[r:KNOWS]->(b)");

        assert!(tokens.contains(&Token::Minus));
        assert!(tokens.contains(&Token::LBracket));
        assert!(tokens.contains(&Token::RBracket));
        assert!(tokens.contains(&Token::DashArrowRight));
    }

    #[test]
    fn test_parameters() {
        let tokens = tokenize("WHERE n.name = $name");

        assert!(tokens
            .iter()
            .any(|t| matches!(t, Token::Parameter(s) if s == "name")));
    }

    #[test]
    fn test_case_insensitive_keywords() {
        let tokens1 = tokenize("match RETURN");
        let tokens2 = tokenize("MATCH return");

        assert!(tokens1.contains(&Token::Match));
        assert!(tokens1.contains(&Token::Return));
        assert!(tokens2.contains(&Token::Match));
        assert!(tokens2.contains(&Token::Return));
    }
}

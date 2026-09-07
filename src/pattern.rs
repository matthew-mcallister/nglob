use std::iter::{FusedIterator, Peekable};

#[derive(Debug, Eq, PartialEq)]
pub enum ParseError {
    InvalidEscape(char),
    IncompleteEscape,
    UnclosedDelimiter(char),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::InvalidEscape(c) => write!(f, r"invalid escape: \{c}"),
            ParseError::IncompleteEscape => write!(f, r"incomplete escape sequence"),
            ParseError::UnclosedDelimiter(c) => write!(f, r"unclosed delimiter: {c}"),
        }
    }
}

impl std::error::Error for ParseError {}

// Distinguishes escaped and unescaped chars
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
enum Token {
    Char(char),
    Question,
    Star,
    StarStar,
    Comma,
    Lbrace,
    Rbrace,
}

fn next_token(input: &mut &str) -> Result<Option<Token>, ParseError> {
    let mut chars = input.chars();
    let tok = match chars.next() {
        Some('\\') => match chars.next() {
            Some(c @ ('*' | '?' | '{' | '}' | ',' | '\\')) => Token::Char(c),
            Some(c) => return Err(ParseError::InvalidEscape(c)),
            None => return Err(ParseError::IncompleteEscape),
        }
        Some('?') => Token::Question,
        Some('*') => if chars.clone().next() == Some('*') {
            chars.next();
            Token::StarStar
        } else {
            Token::Star
        },
        Some(',') => Token::Comma,
        Some('{') => Token::Lbrace,
        Some('}') => Token::Rbrace,
        Some(c) => Token::Char(c),
        None => return Ok(None),
    };
    *input = chars.as_str();
    Ok(Some(tok))
}

#[derive(Clone, Debug)]
struct Tokens<'a> {
    input: &'a str,
}

impl<'a> Iterator for Tokens<'a> {
    type Item = Result<Token, ParseError>;

    fn next(&mut self) -> Option<Self::Item> {
        next_token(&mut self.input).transpose()
    }
}

impl<'a> FusedIterator for Tokens<'a> {}

#[derive(Clone, Debug, Eq, PartialEq)]
enum AstNode {
    Empty,
    Char(char),
    Sequence(Vec<AstNode>),
    Alternative(Vec<AstNode>), // {xxx,yyy,...}
    Wildcard,   // ?
    Star,       // *
    StarStar,   // **
}

fn is_sep(b: u8) -> bool {
    if cfg!(windows) {
        b == b'/' || b == b'\\'
    } else {
        b == b'/'
    }
}

/// # Special symbols
///
/// | Symbol | Meaning |
/// | --- | --- |
/// | `*` | Matches zero or more characters, not including '/' |
/// | `**` | Matches zero or more characters, including '/' |
/// | `?` | Matches any one character |
/// | `{a,b,...}` | Matches exactly one alternative |
/// | `\` | Introduces an escape sequence |
///
/// # Escape sequences
///
/// Possible escape sequences: `\* \? \{ \} \, \\`.
///
/// # Path separators
///
/// On Windows, both `/` and `\\` will be accepted as separators
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Pattern {
    prefix: String,
    root: AstNode,
}

// Parse windows drive prefix (e.g. C:)
fn parse_drive(input: &mut &str) -> String {
    let mut buf = String::new();
    if cfg!(windows) {
        if let Some(b':') = input.as_bytes().get(1) {
            buf.push_str(&input[..2]);
            *input = &input[2..];
        }
    }
    buf
}

fn parse_prefix(input: &mut &str) -> String {
    let mut buf = parse_drive(input);
    while let Some(&b) = input.as_bytes().first() && is_sep(b) {
        buf.push(b as char);
        *input = &input[1..];
    }
    buf
}

// Parses nodes until reaching a stopping condition or end of input.
//
// Will return `Empty` or a single node instead of returning a Sequence node
// with 0 or 1 elements.
fn parse_sequence(
    tokens: &mut Peekable<Tokens>,
    mut stop: impl FnMut(&Token) -> bool,
) -> Result<AstNode, ParseError> {
    let mut nodes = Vec::new();
    loop {
        nodes.push(match tokens.peek() {
            None => break,
            Some(Err(_)) => return Err(tokens.next().unwrap().unwrap_err()),
            Some(Ok(tok)) if stop(tok) => break,
            Some(Ok(_)) => parse_node(tokens)?,
        });
    }
    Ok(match nodes.len() {
        0 => AstNode::Empty,
        1 => nodes.pop().unwrap(),
        _ => AstNode::Sequence(nodes),
    })
}

fn parse_node(tokens: &mut Peekable<Tokens>) -> Result<AstNode, ParseError> {
    let tok = tokens.next().unwrap().unwrap();
    Ok(match tok {
        Token::Char(c) => AstNode::Char(c),
        Token::Question => AstNode::Wildcard,
        Token::Star => AstNode::Star,
        Token::StarStar => AstNode::StarStar,
        Token::Lbrace => parse_alternative(tokens)?,
        Token::Comma => AstNode::Char(','),
        Token::Rbrace => AstNode::Char('}'),
    })
}

fn parse_alternative(tokens: &mut Peekable<Tokens>) -> Result<AstNode, ParseError> {
    let mut branches = Vec::new();
    loop {
        let stop = |tok: &Token| matches!(tok, Token::Comma | Token::Rbrace);
        branches.push(parse_sequence(tokens, stop)?);
        match tokens.next() {
            Some(Ok(Token::Comma)) => {}
            Some(Ok(Token::Rbrace)) => break,
            Some(Ok(_)) => unreachable!(),
            Some(Err(e)) => return Err(e),
            None => return Err(ParseError::UnclosedDelimiter('{')),
        }
    }
    Ok(AstNode::Alternative(branches))
}

pub fn parse(mut input: &str) -> Result<Pattern, ParseError> {
    let prefix = parse_prefix(&mut input);
    let mut input = Tokens { input }.peekable();
    let root = parse_sequence(&mut input, |_| false)?;
    Ok(Pattern {
        prefix,
        root,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use AstNode as A;

    fn chars(s: &str) -> A {
        match s.chars().collect::<Vec<_>>()[..] {
            [] => A::Empty,
            [c] => A::Char(c),
            _ => A::Sequence(s.chars().map(A::Char).collect()),
        }
    }

    #[test]
    fn test_parse() {
        assert_eq!(parse("").unwrap().root, A::Empty);
        assert_eq!(parse("a").unwrap().root, A::Char('a'));
        assert_eq!(parse("abc").unwrap().root, chars("abc"));
        assert_eq!(parse(",}").unwrap().root, chars(",}"));
        assert_eq!(
            parse("a*b").unwrap().root,
            A::Sequence(vec![A::Char('a'), A::Star, A::Char('b')])
        );
        assert_eq!(
            parse("a**b").unwrap().root,
            A::Sequence(vec![A::Char('a'), A::StarStar, A::Char('b')])
        );
        assert_eq!(
            parse("a***b").unwrap().root,
            A::Sequence(vec![A::Char('a'), A::StarStar, A::Star, A::Char('b')])
        );
        assert_eq!(
            parse("{,a,*}").unwrap().root,
            A::Alternative(vec![A::Empty, A::Char('a'), A::Star])
        );
        assert_eq!(
            parse("{{a,b},{c,d}}").unwrap().root,
            A::Alternative(vec![
                A::Alternative(vec![A::Char('a'), A::Char('b')]),
                A::Alternative(vec![A::Char('c'), A::Char('d')]),
            ])
        );
        assert_eq!(
            parse("./*").unwrap().root,
            A::Sequence(vec![A::Char('.'), A::Char('/'), A::Star])
        );
        assert_eq!(parse("..").unwrap().root, chars(".."));
    }

    #[test]
    fn test_failures() {
        assert_eq!(parse("{").unwrap_err(), ParseError::UnclosedDelimiter('{'));
        assert_eq!(parse("{,").unwrap_err(), ParseError::UnclosedDelimiter('{'));
        assert_eq!(parse(r"\a").unwrap_err(), ParseError::InvalidEscape('a'));
        assert_eq!(parse(r"\").unwrap_err(), ParseError::IncompleteEscape);
    }

    #[test]
    fn test_prefix() {
        assert_eq!(parse("/").unwrap().prefix, "/");
        assert_eq!(parse("/home").unwrap().prefix, "/");
        assert_eq!(parse("//").unwrap().prefix, "//");
    }

    #[cfg(windows)]
    #[test]
    fn test_prefix_windows() {
        assert_eq!(parse(r"\\").unwrap().prefix, r"\\");
        assert_eq!(parse(r"\\\\").unwrap().prefix, r"\\\\");
        assert_eq!(parse("c:").unwrap().prefix, "c:");
        assert_eq!(parse("c:/").unwrap().prefix, "c:/");
        assert_eq!(parse(r"c:\\").unwrap().prefix, r"c:\\");
        assert_eq!(parse(r"c:\\User\\").unwrap().prefix, r"c:\\User\\");
    }

    #[cfg(not(windows))]
    #[test]
    fn test_prefix_unix() {
        assert_eq!(parse(r"\\").unwrap().prefix, "");
        assert_eq!(parse(r"\\\\").unwrap().prefix, "");
        assert_eq!(parse("c:").unwrap().prefix, "");
        assert_eq!(parse("c:/").unwrap().prefix, "");
        assert_eq!(parse(r"c:\\").unwrap().prefix, "");
        assert_eq!(parse(r"c:\\User\\").unwrap().prefix, "");
    }
}

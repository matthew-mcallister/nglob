use std::fmt::Write;
use std::iter::FusedIterator;
use std::path::is_separator;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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
    Sep,
}

impl std::fmt::Display for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Token::Char(c) => write!(f, "{}", c),
            Token::Question => write!(f, "?"),
            Token::Star => write!(f, "*"),
            Token::StarStar => write!(f, "**"),
            Token::Comma => write!(f, ","),
            Token::Lbrace => write!(f, "{{"),
            Token::Rbrace => write!(f, "}}"),
            Token::Sep => write!(f, "{}", std::path::MAIN_SEPARATOR),
        }
    }
}

fn next_token(input: &mut &str) -> Result<Option<Token>, ParseError> {
    let mut chars = input.chars();
    let tok = match chars.next() {
        Some('\\') => match chars.next() {
            Some(c @ ('*' | '?' | '{' | '}' | ',')) => Token::Char(c),
            #[cfg(windows)]
            Some('\\') => Token::Sep,
            #[cfg(not(windows))]
            Some('\\') => Token::Char('\\'),
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
        Some(c) if is_separator(c) => Token::Sep,
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

impl<'a> Tokens<'a> {
    fn peek(&self) -> Option<Result<Token, ParseError>> {
        self.clone().next()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Ast {
    Empty,
    Char(char),
    Sep,
    Sequence(Vec<Ast>),
    Alternative(Vec<Ast>), // {xxx,yyy,...}
    Wildcard,   // ?
    Star,       // *
    StarStar,   // **
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
    /// Base directory for search
    pub base: String,
    /// Root of AST
    pub root: Ast,
}

#[cfg(windows)]
fn is_prefix_sep(c: char) -> bool {
    c == ':'
}

#[cfg(not(windows))]
fn is_prefix_sep(_: char) -> bool {
    false
}

// Returns a filepath to use as the search base. Looks for the longest literal
// string starting from the beginning that ends in / (or : or \ on Windows).
// This skips a lot of unnecessary reads and handles drive prefixes and UNC on
// Windows.
fn parse_base(input: &mut Tokens<'_>) -> Result<String, ParseError> {
    let mut prefix = String::new();
    let mut buffer = String::new();
    let mut lookahead = input.clone();
    loop {
        let Some(tok) = lookahead.next().transpose()? else {
            *input = lookahead.clone();
            prefix.push_str(&buffer);
            return Ok(prefix);
        };

        let flush = match tok {
            Token::Sep => true,
            Token::Char(c) if is_prefix_sep(c) => true,
            Token::Char(_) => false,
            _ => return Ok(prefix),
        };
        let _ = write!(&mut buffer, "{}", tok);

        if flush {
            *input = lookahead.clone();
            prefix.push_str(&buffer);
            buffer.clear();
        }
    }
}

// Parses nodes until reaching a stopping condition or end of input.
//
// Will return `Empty` or a single node instead of returning a Sequence node
// with 0 or 1 elements.
fn parse_sequence(
    tokens: &mut Tokens,
    mut stop: impl FnMut(&Token) -> bool,
) -> Result<Ast, ParseError> {
    let mut nodes = Vec::new();
    loop {
        nodes.push(match tokens.peek() {
            None => break,
            Some(Err(_)) => return Err(tokens.next().unwrap().unwrap_err()),
            Some(Ok(tok)) if stop(&tok) => break,
            Some(Ok(_)) => parse_node(tokens)?,
        });
    }
    Ok(match nodes.len() {
        0 => Ast::Empty,
        1 => nodes.pop().unwrap(),
        _ => Ast::Sequence(nodes),
    })
}

fn parse_node(tokens: &mut Tokens) -> Result<Ast, ParseError> {
    let tok = tokens.next().unwrap().unwrap();
    Ok(match tok {
        Token::Char(c) => Ast::Char(c),
        Token::Sep => Ast::Sep,
        Token::Question => Ast::Wildcard,
        Token::Star => Ast::Star,
        Token::StarStar => Ast::StarStar,
        Token::Lbrace => parse_alternative(tokens)?,
        Token::Comma => Ast::Char(','),
        Token::Rbrace => Ast::Char('}'),
    })
}

fn parse_alternative(tokens: &mut Tokens) -> Result<Ast, ParseError> {
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
    Ok(Ast::Alternative(branches))
}

pub fn parse_ast(input: &str) -> Result<Ast, ParseError> {
    parse_sequence(&mut Tokens { input }, |_| false)
}

pub fn parse(input: &str) -> Result<Pattern, ParseError> {
    let mut input = Tokens { input };
    let base = parse_base(&mut input)?;
    let root = parse_sequence(&mut input, |_| false)?;
    Ok(Pattern {
        base,
        root,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use Ast::*;

    fn parse_seq(s: &str) -> Ast {
        parse_ast(s).unwrap()
    }

    fn pattern(base: &str, root: Ast) -> Pattern {
        Pattern {
            base: base.into(),
            root,
        }
    }

    #[test]
    fn test_parse() {
        assert_eq!(parse_seq(""), Empty);
        assert_eq!(parse_seq("a"), Char('a'));
        assert_eq!(parse_seq("abc"), Sequence(vec![Char('a'), Char('b'), Char('c')]));
        assert_eq!(parse_seq(",}"), Sequence(vec![Char(','), Char('}')]));
        assert_eq!(parse_seq("a*b"), Sequence(vec![Char('a'), Star, Char('b')]));
        assert_eq!(parse_seq("a**b"), Sequence(vec![Char('a'), StarStar, Char('b')]));
        assert_eq!(parse_seq("a***b"), Sequence(vec![Char('a'), StarStar, Star, Char('b')]));
        assert_eq!(parse_seq("{,a,*}"), Alternative(vec![Empty, Char('a'), Star]));
        assert_eq!(
            parse_seq("{{a,b},{c,d}}"),
            Alternative(vec![
                Alternative(vec![Char('a'), Char('b')]),
                Alternative(vec![Char('c'), Char('d')]),
            ])
        );
        assert_eq!(parse_seq("./*"), Sequence(vec![Char('.'), Sep, Star]));
        assert_eq!(parse_seq("*/."), Sequence(vec![Star, Sep, Char('.')]));
        assert_eq!(parse_seq(".."), Sequence(vec![Char('.'), Char('.')]));
    }

    #[test]
    fn test_parse_base() {
        assert_eq!(parse("..").unwrap(), pattern("..", Empty));

        #[cfg(not(windows))]
        {
            assert_eq!(parse("/home").unwrap(), pattern("/home", Empty));
            assert_eq!(parse("/home/*").unwrap(), pattern("/home/", Star));
            assert_eq!(parse("///").unwrap(), pattern("///", Empty));
            assert_eq!(parse("//Host/share/*").unwrap(), pattern("//Host/share/", Star));
            assert_eq!(parse("./*").unwrap(), pattern("./", Star));

            assert_eq!(parse(r".\\*").unwrap(), pattern("", Sequence(vec![Char('.'), Char('\\'), Star])));
            assert_eq!(parse(r"C:*").unwrap(), pattern("", Sequence(vec![Char('C'), Char(':'), Star])));
        }

        #[cfg(windows)]
        {
            assert_eq!(parse("/home").unwrap(), pattern(r"\home", Empty));
            assert_eq!(parse("/home/*").unwrap(), pattern(r"\home\", Star));
            assert_eq!(parse("///").unwrap(), pattern(r"\\\", Empty));
            assert_eq!(parse("//Host/share/*").unwrap(), pattern(r"\\Host\share\", Star));
            assert_eq!(parse("./*").unwrap(), pattern(r".\", Star));

            assert_eq!(parse(r".\\*").unwrap(), pattern(r".\", Star));
            assert_eq!(parse(r"C:*").unwrap(), pattern("C:", Star));

            assert_eq!(parse(r"C:\\").unwrap(), pattern(r"C:\", Empty));
            assert_eq!(parse(r"C:\\Program Files\\").unwrap(), pattern(r"C:\Program Files\", Empty));
            assert_eq!(parse("C:/").unwrap(), pattern(r"C:\", Empty));
            assert_eq!(parse(r"\\\\Host\\share\\*").unwrap(), pattern(r"\\Host\share\", Star));
        }
    }

    #[test]
    fn test_failures() {
        assert_eq!(parse("{").unwrap_err(), ParseError::UnclosedDelimiter('{'));
        assert_eq!(parse("{,").unwrap_err(), ParseError::UnclosedDelimiter('{'));
        assert_eq!(parse(r"\a").unwrap_err(), ParseError::InvalidEscape('a'));
        assert_eq!(parse(r"\").unwrap_err(), ParseError::IncompleteEscape);
    }
}

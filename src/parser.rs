use std::collections::HashMap;

/// A parse failure, tied to the exact line and column where it happened so
/// the caller can print a caret pointing at the offending character.
#[derive(Debug)]
pub struct ParseError {
    pub line: usize,
    pub col: usize,
    pub message: String,
}

impl ParseError {
    fn new(line: usize, col: usize, message: impl Into<String>) -> Self {
        ParseError {
            line,
            col,
            message: message.into(),
        }
    }

    /// Render a compiler-style snippet: the offending line plus a caret
    /// under the exact column, so a bad grammar file is easy to fix without
    /// re-reading it character by character.
    pub fn render(&self, source: &str) -> String {
        let line_text = source.lines().nth(self.line.saturating_sub(1)).unwrap_or("");
        let line_num = self.line.to_string();
        let gutter = " ".repeat(line_num.len());
        let caret_pad = " ".repeat(self.col.saturating_sub(1));
        format!(
            "error: {msg}\n{gutter} |\n{line_num} | {line_text}\n{gutter} | {caret_pad}^",
            msg = self.message,
        )
    }
}

#[derive(Debug, Clone)]
pub enum Part {
    Literal(String),
    Reference { name: String, line: usize, col: usize },
}

/// One branch of a rule, plus how often it should be picked relative to its
/// siblings. A bare alternative defaults to weight 1; `"Stark":3` is three
/// times as likely to be chosen as a default-weight sibling.
#[derive(Debug, Clone)]
pub struct Alternative {
    pub parts: Vec<Part>,
    pub weight: u32,
}

pub struct Grammar {
    rules: HashMap<String, Vec<Alternative>>,
}

impl Grammar {
    pub fn get(&self, name: &str) -> Option<&Vec<Alternative>> {
        self.rules.get(name)
    }
}

struct Scanner {
    chars: Vec<char>,
    pos: usize,
    line: usize,
    col: usize,
}

impl Scanner {
    fn new(source: &str) -> Self {
        Scanner {
            chars: source.chars().collect(),
            pos: 0,
            line: 1,
            col: 1,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += 1;
        if c == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(c)
    }

    fn error(&self, message: impl Into<String>) -> ParseError {
        ParseError::new(self.line, self.col, message)
    }

    /// Skips whitespace of any kind (including newlines) and '#' comments.
    /// Used between rules, where line breaks carry no meaning.
    fn skip_blank_and_comments(&mut self) {
        loop {
            match self.peek() {
                Some(c) if c.is_whitespace() => {
                    self.advance();
                }
                Some('#') => {
                    while let Some(c) = self.peek() {
                        if c == '\n' {
                            break;
                        }
                        self.advance();
                    }
                }
                _ => break,
            }
        }
    }

    /// Skips spaces and tabs only, not newlines. Used inside a single
    /// alternative, where a bare newline ends it.
    fn skip_inline_space(&mut self) {
        while let Some(c) = self.peek() {
            if c == ' ' || c == '\t' {
                self.advance();
            } else {
                break;
            }
        }
    }

    fn read_identifier(&mut self) -> Result<String, ParseError> {
        let mut ident = String::new();
        match self.peek() {
            Some(c) if c.is_alphabetic() || c == '_' => {
                ident.push(c);
                self.advance();
            }
            _ => return Err(self.error("expected a rule name (letters, digits, underscores)")),
        }
        while let Some(c) = self.peek() {
            if c.is_alphanumeric() || c == '_' {
                ident.push(c);
                self.advance();
            } else {
                break;
            }
        }
        Ok(ident)
    }

    fn read_string(&mut self) -> Result<String, ParseError> {
        let start_line = self.line;
        let start_col = self.col;
        self.advance(); // opening quote
        let mut value = String::new();
        loop {
            match self.peek() {
                None | Some('\n') => {
                    return Err(ParseError::new(
                        start_line,
                        start_col,
                        "unterminated string literal, expected a closing '\"'",
                    ));
                }
                Some('\\') => {
                    self.advance();
                    match self.peek() {
                        Some('"') => {
                            value.push('"');
                            self.advance();
                        }
                        Some('\\') => {
                            value.push('\\');
                            self.advance();
                        }
                        Some('n') => {
                            value.push('\n');
                            self.advance();
                        }
                        Some(other) => {
                            return Err(self.error(format!(
                                "unknown escape sequence '\\{}', expected \\\" or \\\\",
                                other
                            )));
                        }
                        None => {
                            return Err(self.error("unterminated escape sequence at end of file"));
                        }
                    }
                }
                Some('"') => {
                    self.advance();
                    break;
                }
                Some(c) => {
                    value.push(c);
                    self.advance();
                }
            }
        }
        Ok(value)
    }

    fn read_reference(&mut self) -> Result<Part, ParseError> {
        let line = self.line;
        let col = self.col;
        self.advance(); // '<'
        if !matches!(self.peek(), Some(c) if c.is_alphabetic() || c == '_') {
            return Err(self.error("expected a rule name after '<'"));
        }
        let name = self.read_identifier()?;
        match self.peek() {
            Some('>') => {
                self.advance();
            }
            _ => {
                return Err(ParseError::new(
                    line,
                    col,
                    "unterminated rule reference, expected a closing '>'",
                ));
            }
        }
        Ok(Part::Reference { name, line, col })
    }

    /// Reads a `:N` weight suffix. The ':' has already been peeked but not
    /// consumed.
    fn read_weight(&mut self) -> Result<u32, ParseError> {
        let line = self.line;
        let col = self.col;
        self.advance(); // ':'
        let mut digits = String::new();
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                digits.push(c);
                self.advance();
            } else {
                break;
            }
        }
        if digits.is_empty() {
            return Err(self.error("expected a number after ':'"));
        }
        let weight: u32 = digits
            .parse()
            .map_err(|_| ParseError::new(line, col, format!("weight '{}' is too large", digits)))?;
        if weight == 0 {
            return Err(ParseError::new(line, col, "weight must be at least 1, got 0"));
        }
        Ok(weight)
    }
}

fn parse_alternative(scanner: &mut Scanner) -> Result<Alternative, ParseError> {
    let start_line = scanner.line;
    let start_col = scanner.col;
    let mut parts = Vec::new();
    let mut weight = None;

    loop {
        scanner.skip_inline_space();
        match scanner.peek() {
            None | Some('\n') | Some('|') | Some('#') => break,
            Some('"') => {
                let text = scanner.read_string()?;
                parts.push(Part::Literal(text));
            }
            Some('<') => {
                let reference = scanner.read_reference()?;
                parts.push(reference);
            }
            Some(':') => {
                if weight.is_some() {
                    return Err(scanner.error("an alternative can only have one weight"));
                }
                weight = Some(scanner.read_weight()?);
            }
            Some(c) => {
                return Err(scanner.error(format!(
                    "unexpected character '{}', expected a quoted string, a <rule-reference>, or a ':weight'",
                    c
                )));
            }
        }
    }

    if parts.is_empty() {
        return Err(ParseError::new(
            start_line,
            start_col,
            "empty alternative, expected a quoted string or a <rule-reference>",
        ));
    }

    Ok(Alternative {
        parts,
        weight: weight.unwrap_or(1),
    })
}

/// Parses a grammar file. A grammar is a set of rules of the form
/// `name: alternative | alternative | ...`, where each alternative is a
/// sequence of quoted string literals and `<other-rule>` references. One
/// rule must be named `root`; generation starts there.
pub fn parse(source: &str) -> Result<Grammar, ParseError> {
    let mut scanner = Scanner::new(source);
    let mut rules: HashMap<String, Vec<Alternative>> = HashMap::new();
    let mut rule_positions: HashMap<String, (usize, usize)> = HashMap::new();

    loop {
        scanner.skip_blank_and_comments();
        if scanner.peek().is_none() {
            break;
        }

        let name_line = scanner.line;
        let name_col = scanner.col;
        let name = scanner.read_identifier()?;

        scanner.skip_inline_space();
        match scanner.peek() {
            Some(':') => {
                scanner.advance();
            }
            _ => {
                return Err(scanner.error(format!("expected ':' after rule name '{}'", name)));
            }
        }

        let mut alternatives = Vec::new();
        loop {
            scanner.skip_inline_space();
            let alt = parse_alternative(&mut scanner)?;
            alternatives.push(alt);

            scanner.skip_blank_and_comments();
            if scanner.peek() == Some('|') {
                scanner.advance();
                continue;
            }
            break;
        }

        if let Some((prev_line, prev_col)) = rule_positions.get(&name) {
            return Err(ParseError::new(
                name_line,
                name_col,
                format!(
                    "rule '{}' is already defined at line {}, column {}",
                    name, prev_line, prev_col
                ),
            ));
        }
        rule_positions.insert(name.clone(), (name_line, name_col));
        rules.insert(name, alternatives);
    }

    if rules.is_empty() {
        return Err(ParseError::new(
            1,
            1,
            "grammar file is empty, expected at least a 'root' rule",
        ));
    }

    if !rules.contains_key("root") {
        return Err(ParseError::new(
            scanner.line,
            1,
            "grammar has no rule named 'root' (every grammar needs a starting rule called 'root')",
        ));
    }

    for alts in rules.values() {
        for alt in alts {
            for part in &alt.parts {
                if let Part::Reference { name, line, col } = part {
                    if !rules.contains_key(name) {
                        return Err(ParseError::new(
                            *line,
                            *col,
                            format!("rule '<{}>' is not defined anywhere in this grammar", name),
                        ));
                    }
                }
            }
        }
    }

    Ok(Grammar { rules })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expect_error(source: &str) -> ParseError {
        parse(source).expect_err("expected a parse error")
    }

    #[test]
    fn empty_file_reports_start_of_file() {
        let err = expect_error("");
        assert_eq!((err.line, err.col), (1, 1));
    }

    #[test]
    fn missing_root_rule_points_at_end_of_file() {
        let err = expect_error("foo: \"a\"\n");
        assert_eq!((err.line, err.col), (2, 1));
    }

    #[test]
    fn duplicate_rule_name_points_at_second_definition() {
        let err = expect_error("root: \"a\"\nroot: \"b\"\n");
        assert_eq!((err.line, err.col), (2, 1));
        assert!(err.message.contains("already defined at line 1, column 1"));
    }

    #[test]
    fn missing_colon_after_rule_name() {
        let err = expect_error("root \"a\"\n");
        assert_eq!((err.line, err.col), (1, 6));
    }

    #[test]
    fn empty_alternative_points_at_its_start() {
        let err = expect_error("root: | \"a\"\n");
        assert_eq!((err.line, err.col), (1, 7));
    }

    #[test]
    fn unterminated_string_points_at_opening_quote() {
        let err = expect_error("root: \"abc\n");
        assert_eq!((err.line, err.col), (1, 7));
    }

    #[test]
    fn unknown_escape_sequence() {
        let err = expect_error("root: \"a\\qb\"\n");
        assert_eq!((err.line, err.col), (1, 10));
    }

    #[test]
    fn unterminated_escape_at_end_of_file() {
        let err = expect_error("root: \"a\\");
        assert_eq!((err.line, err.col), (1, 10));
    }

    #[test]
    fn reference_missing_name() {
        let err = expect_error("root: <>\n");
        assert_eq!((err.line, err.col), (1, 8));
    }

    #[test]
    fn unterminated_reference_points_at_opening_bracket() {
        let err = expect_error("root: <first\n");
        assert_eq!((err.line, err.col), (1, 7));
    }

    #[test]
    fn weight_missing_digits() {
        let err = expect_error("root: \"a\": \n");
        assert_eq!((err.line, err.col), (1, 11));
    }

    #[test]
    fn weight_overflow() {
        let err = expect_error("root: \"a\":9999999999\n");
        assert_eq!((err.line, err.col), (1, 10));
    }

    #[test]
    fn weight_zero_is_rejected() {
        let err = expect_error("root: \"a\":0\n");
        assert_eq!((err.line, err.col), (1, 10));
    }

    #[test]
    fn duplicate_weight_on_one_alternative() {
        let err = expect_error("root: \"a\":1:2\n");
        assert_eq!((err.line, err.col), (1, 12));
    }

    #[test]
    fn unexpected_character() {
        let err = expect_error("root: %\n");
        assert_eq!((err.line, err.col), (1, 7));
    }

    #[test]
    fn undefined_reference_points_at_the_reference() {
        let err = expect_error("root: <missing>\n");
        assert_eq!((err.line, err.col), (1, 7));
    }

    #[test]
    fn rule_name_must_start_with_a_letter_or_underscore() {
        let err = expect_error("123abc: \"a\"\n");
        assert_eq!((err.line, err.col), (1, 1));
    }

    #[test]
    fn parses_a_well_formed_grammar() {
        let grammar = parse(
            "root: <first> \" \" <last>\n\nfirst: \"Bran\" | \"Eddard\"\n\nlast: \"Stark\":3 | \"Snow\"\n",
        )
        .expect("well-formed grammar should parse");
        assert_eq!(grammar.get("root").unwrap().len(), 1);
        assert_eq!(grammar.get("last").unwrap()[0].weight, 3);
    }
}

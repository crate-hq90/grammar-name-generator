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

pub struct Grammar {
    rules: HashMap<String, Vec<Vec<Part>>>,
}

impl Grammar {
    pub fn get(&self, name: &str) -> Option<&Vec<Vec<Part>>> {
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
}

fn parse_alternative(scanner: &mut Scanner) -> Result<Vec<Part>, ParseError> {
    let start_line = scanner.line;
    let start_col = scanner.col;
    let mut parts = Vec::new();

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
            Some(c) => {
                return Err(scanner.error(format!(
                    "unexpected character '{}', expected a quoted string or a <rule-reference>",
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

    Ok(parts)
}

/// Parses a grammar file. A grammar is a set of rules of the form
/// `name: alternative | alternative | ...`, where each alternative is a
/// sequence of quoted string literals and `<other-rule>` references. One
/// rule must be named `root`; generation starts there.
pub fn parse(source: &str) -> Result<Grammar, ParseError> {
    let mut scanner = Scanner::new(source);
    let mut rules: HashMap<String, Vec<Vec<Part>>> = HashMap::new();
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
            for part in alt {
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

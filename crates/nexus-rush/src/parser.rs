//! Build a `CommandList` from a token stream via recursive descent.
//!
//! Grammar (v0):
//!   list     := and_or ( sep and_or )* sep?          sep = ; | & | newline
//!   and_or   := pipeline ( ('&&' | '||') pipeline )*
//!   pipeline := command ( '|' command )*
//!   command  := compound | simple
//!   compound := if_clause | loop_clause | for_clause
//!   simple   := (word | redirect)+
//!   redirect := ('<' | '>' | '>>') word
//!
//!   if_clause   := 'if' list 'then' list ('elif' list 'then' list)* ('else' list)? 'fi'
//!   loop_clause := ('while' | 'until') list 'do' list 'done'
//!   for_clause  := 'for' NAME ('in' word*)? sep 'do' list 'done'
//!
//! `&&`/`||` bind pipelines into one job; `;`/`&`/newline separate jobs, with
//! `&` backgrounding the preceding job. Reserved words (`if`, `then`, …) are
//! recognised only in command position; elsewhere they are ordinary words.
//!
//! "Raw" here means words still carry their quoting (see [`crate::lexer::Word`])
//! and are *not* expanded; expansion happens lazily as the list runs.

use std::fmt;

use crate::lexer::{self, RedirOp, Token, Word, WordPart};

/// A list: jobs separated by `;`/`&`/newline.
#[derive(Debug, Clone)]
pub struct CommandList {
    pub jobs: Vec<Job>,
}

/// One job — an and-or chain — plus whether it runs in the background (`&`).
#[derive(Debug, Clone)]
pub struct Job {
    pub list: AndOrList,
    pub background: bool,
}

/// Pipelines joined by `&&`/`||`, run left-to-right.
#[derive(Debug, Clone)]
pub struct AndOrList {
    pub first: RawPipeline,
    pub rest: Vec<(Connector, RawPipeline)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Connector {
    /// `&&` — run the next pipeline only if the previous one succeeded.
    And,
    /// `||` — run the next pipeline only if the previous one failed.
    Or,
}

#[derive(Debug, Clone)]
pub struct RawPipeline {
    pub commands: Vec<RawCommand>,
}

/// A pipeline stage: either a plain command or a compound (`if`/`while`/`for`).
#[derive(Debug, Clone)]
pub enum RawCommand {
    Simple(RawSimple),
    Compound(Box<Compound>),
}

#[derive(Debug, Clone)]
pub struct RawSimple {
    pub argv: Vec<Word>,
    pub redirects: Vec<RawRedirect>,
}

#[derive(Debug, Clone)]
pub enum RawRedirect {
    /// `[fd]< file` / `[fd]> file` / `[fd]>> file`.
    File {
        fd: u32,
        file: Word,
        mode: RedirMode,
    },
    /// `&> file` / `&>> file` — stdout and stderr to one file.
    Both { file: Word, append: bool },
    /// `fd>&target` — `fd` duplicates `target` (e.g. `2>&1`).
    Dup { fd: u32, target: u32 },
    /// `<<DELIM` here-document; `body` is the collected text, expanded unless the
    /// delimiter was quoted.
    Heredoc { body: String, expand: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedirMode {
    Read,
    Write,
    Append,
}

/// A compound command. Each body is itself a list, run by the executor.
#[derive(Debug, Clone)]
pub enum Compound {
    /// `if` with its `elif`s flattened into `branches` of `(condition, body)`.
    If {
        branches: Vec<(CommandList, CommandList)>,
        else_body: Option<CommandList>,
    },
    /// `while` (or `until`, when `until` is set).
    Loop {
        until: bool,
        cond: CommandList,
        body: CommandList,
    },
    /// `for NAME in WORDS; do BODY; done`.
    For {
        var: String,
        words: Vec<Word>,
        body: CommandList,
    },
    /// `case WORD in PATTERN|… ) BODY ;; … esac`.
    Case {
        word: Word,
        items: Vec<(Vec<Word>, CommandList)>,
    },
    /// A brace group `{ list; }` — runs the list in the current shell.
    Group(CommandList),
    /// A subshell `( list )` — runs the list with isolated cwd and variables.
    Subshell(CommandList),
    /// A function definition `name() { list; }`.
    FuncDef { name: String, body: CommandList },
}

/// A parse failure. `Incomplete` means the input is a valid prefix that needs
/// more lines (the REPL keeps reading); `Syntax` is a real error.
#[derive(Debug)]
pub enum ParseError {
    Incomplete,
    Syntax(String),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::Incomplete => write!(f, "unexpected end of input"),
            ParseError::Syntax(msg) => write!(f, "{msg}"),
        }
    }
}

const RESERVED: &[&str] = &[
    "if", "then", "elif", "else", "fi", "while", "until", "do", "done", "for", "in", "case",
    "esac", "{", "}",
];

pub fn parse(input: &str) -> Result<CommandList, ParseError> {
    let tokens = lexer::lex(input).map_err(|e| match e {
        lexer::LexError::Incomplete => ParseError::Incomplete,
        lexer::LexError::Syntax(msg) => ParseError::Syntax(msg),
    })?;
    let mut p = Parser {
        toks: tokens,
        pos: 0,
    };

    let list = p.parse_list()?;
    p.skip_separators();
    if let Some(tok) = p.peek() {
        return Err(ParseError::Syntax(format!(
            "unexpected `{}`",
            describe(tok)
        )));
    }
    Ok(list)
}

struct Parser {
    toks: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.toks.get(self.pos)
    }

    fn advance(&mut self) -> Option<Token> {
        let tok = self.toks.get(self.pos).cloned();
        if tok.is_some() {
            self.pos += 1;
        }
        tok
    }

    fn at_end(&self) -> bool {
        self.pos >= self.toks.len()
    }

    /// The reserved word at the cursor, if the current token is one.
    fn peek_keyword(&self) -> Option<&'static str> {
        self.peek().and_then(as_keyword)
    }

    /// Skip `;` and newline separators (not `&`, which is significant).
    fn skip_separators(&mut self) {
        while matches!(self.peek(), Some(Token::Semi | Token::Newline)) {
            self.pos += 1;
        }
    }

    /// Skip newlines only — used after `&&`/`||`/`|` to allow line continuation.
    fn skip_newlines(&mut self) {
        while matches!(self.peek(), Some(Token::Newline)) {
            self.pos += 1;
        }
    }

    /// A list ends at end of input or a reserved word that closes a construct
    /// (`then`, `fi`, `do`, …) — anything that isn't a command starter.
    fn at_list_end(&self) -> bool {
        // `;;` closes a `case` item's body; `)` closes a subshell.
        if matches!(self.peek(), Some(Token::DSemi | Token::RParen)) {
            return true;
        }
        match self.peek_keyword() {
            Some(kw) => !is_command_start(kw),
            None => self.at_end(),
        }
    }

    fn parse_list(&mut self) -> Result<CommandList, ParseError> {
        let mut jobs = Vec::new();
        loop {
            self.skip_separators();
            if self.at_list_end() {
                break;
            }
            let list = self.parse_and_or()?;
            let background = matches!(self.peek(), Some(Token::Amp));
            if matches!(self.peek(), Some(Token::Semi | Token::Amp | Token::Newline)) {
                self.pos += 1;
            }
            jobs.push(Job { list, background });
        }
        Ok(CommandList { jobs })
    }

    fn parse_and_or(&mut self) -> Result<AndOrList, ParseError> {
        let first = self.parse_pipeline()?;
        let mut rest = Vec::new();
        loop {
            let connector = match self.peek() {
                Some(Token::And) => Connector::And,
                Some(Token::Or) => Connector::Or,
                _ => break,
            };
            self.pos += 1;
            self.skip_newlines();
            rest.push((connector, self.parse_pipeline()?));
        }
        Ok(AndOrList { first, rest })
    }

    fn parse_pipeline(&mut self) -> Result<RawPipeline, ParseError> {
        let mut commands = vec![self.parse_command()?];
        while matches!(self.peek(), Some(Token::Pipe)) {
            self.pos += 1;
            self.skip_newlines();
            commands.push(self.parse_command()?);
        }
        Ok(RawPipeline { commands })
    }

    fn parse_command(&mut self) -> Result<RawCommand, ParseError> {
        // `name ( )` in command position is a function definition.
        if self.is_funcdef_ahead() {
            return self.parse_funcdef();
        }
        // A bare `(` starts a subshell.
        if matches!(self.peek(), Some(Token::LParen)) {
            return self.parse_subshell();
        }
        match self.peek_keyword() {
            Some("if") => self.parse_if(),
            Some("while") => self.parse_loop(false),
            Some("until") => self.parse_loop(true),
            Some("for") => self.parse_for(),
            Some("case") => self.parse_case(),
            Some("{") => self.parse_group(),
            // A closing keyword here means a body was empty (e.g. `if; then`).
            Some(kw) => Err(ParseError::Syntax(format!("unexpected `{kw}`"))),
            None => self.parse_simple(),
        }
    }

    /// True if the cursor is at `NAME ( )` — a function definition.
    fn is_funcdef_ahead(&self) -> bool {
        let is_name_word = matches!(self.toks.get(self.pos),
            Some(Token::Word(parts)) if matches!(parts.as_slice(), [WordPart::Unquoted(s)] if is_name(s)));
        is_name_word
            && matches!(self.toks.get(self.pos + 1), Some(Token::LParen))
            && matches!(self.toks.get(self.pos + 2), Some(Token::RParen))
    }

    fn parse_funcdef(&mut self) -> Result<RawCommand, ParseError> {
        let name = self.expect_name()?;
        self.pos += 2; // `(` `)` — guaranteed by is_funcdef_ahead
        self.skip_newlines();
        let body = self.parse_brace_body()?;
        Ok(RawCommand::Compound(Box::new(Compound::FuncDef {
            name,
            body,
        })))
    }

    fn parse_group(&mut self) -> Result<RawCommand, ParseError> {
        let list = self.parse_brace_body()?;
        Ok(RawCommand::Compound(Box::new(Compound::Group(list))))
    }

    fn parse_subshell(&mut self) -> Result<RawCommand, ParseError> {
        self.pos += 1; // `(`
        let list = self.parse_list()?;
        match self.peek() {
            Some(Token::RParen) => self.pos += 1,
            None => return Err(ParseError::Incomplete),
            _ => return Err(ParseError::Syntax("expected `)`".into())),
        }
        Ok(RawCommand::Compound(Box::new(Compound::Subshell(list))))
    }

    /// Parse `{ list; }`.
    fn parse_brace_body(&mut self) -> Result<CommandList, ParseError> {
        self.expect_keyword("{")?;
        let list = self.parse_list()?;
        self.expect_keyword("}")?;
        Ok(list)
    }

    fn parse_simple(&mut self) -> Result<RawCommand, ParseError> {
        let mut argv = Vec::new();
        let mut redirects = Vec::new();

        loop {
            match self.peek() {
                // After the first word, reserved words are ordinary arguments,
                // so we match on `Word` without consulting `as_keyword`.
                Some(Token::Word(_)) => {
                    let Some(Token::Word(w)) = self.advance() else {
                        unreachable!()
                    };
                    argv.push(w);
                }
                Some(Token::Redirect(_)) => {
                    let Some(Token::Redirect(r)) = self.advance() else {
                        unreachable!()
                    };
                    redirects.push(match r.op {
                        RedirOp::Read => RawRedirect::File {
                            fd: r.fd,
                            file: self.expect_word("<")?,
                            mode: RedirMode::Read,
                        },
                        RedirOp::Write => RawRedirect::File {
                            fd: r.fd,
                            file: self.expect_word(">")?,
                            mode: RedirMode::Write,
                        },
                        RedirOp::Append => RawRedirect::File {
                            fd: r.fd,
                            file: self.expect_word(">>")?,
                            mode: RedirMode::Append,
                        },
                        RedirOp::Both => RawRedirect::Both {
                            file: self.expect_word("&>")?,
                            append: false,
                        },
                        RedirOp::BothAppend => RawRedirect::Both {
                            file: self.expect_word("&>>")?,
                            append: true,
                        },
                        RedirOp::Dup(target) => RawRedirect::Dup { fd: r.fd, target },
                        RedirOp::Heredoc { body, expand } => RawRedirect::Heredoc { body, expand },
                    });
                }
                _ => break,
            }
        }

        if argv.is_empty() && redirects.is_empty() {
            return Err(self.eof_or_syntax("expected a command"));
        }
        Ok(RawCommand::Simple(RawSimple { argv, redirects }))
    }

    fn parse_if(&mut self) -> Result<RawCommand, ParseError> {
        self.expect_keyword("if")?;
        let mut branches = Vec::new();
        branches.push(self.parse_cond_then()?);
        while self.peek_keyword() == Some("elif") {
            self.pos += 1;
            branches.push(self.parse_cond_then()?);
        }
        let else_body = if self.peek_keyword() == Some("else") {
            self.pos += 1;
            Some(self.parse_list()?)
        } else {
            None
        };
        self.expect_keyword("fi")?;
        Ok(RawCommand::Compound(Box::new(Compound::If {
            branches,
            else_body,
        })))
    }

    fn parse_cond_then(&mut self) -> Result<(CommandList, CommandList), ParseError> {
        let cond = self.parse_list()?;
        self.expect_keyword("then")?;
        let body = self.parse_list()?;
        Ok((cond, body))
    }

    fn parse_loop(&mut self, until: bool) -> Result<RawCommand, ParseError> {
        self.expect_keyword(if until { "until" } else { "while" })?;
        let cond = self.parse_list()?;
        self.expect_keyword("do")?;
        let body = self.parse_list()?;
        self.expect_keyword("done")?;
        Ok(RawCommand::Compound(Box::new(Compound::Loop {
            until,
            cond,
            body,
        })))
    }

    fn parse_for(&mut self) -> Result<RawCommand, ParseError> {
        self.expect_keyword("for")?;
        let var = self.expect_name()?;

        let mut words = Vec::new();
        if self.peek_keyword() == Some("in") {
            self.pos += 1;
            while let Some(Token::Word(_)) = self.peek() {
                let Some(Token::Word(w)) = self.advance() else {
                    unreachable!()
                };
                words.push(w);
            }
        }

        // A separator (`;` or newline) precedes `do`.
        self.skip_separators();
        self.expect_keyword("do")?;
        let body = self.parse_list()?;
        self.expect_keyword("done")?;
        Ok(RawCommand::Compound(Box::new(Compound::For {
            var,
            words,
            body,
        })))
    }

    fn parse_case(&mut self) -> Result<RawCommand, ParseError> {
        self.expect_keyword("case")?;
        let word = self.expect_word_token()?;
        self.expect_keyword("in")?;

        let mut items = Vec::new();
        loop {
            self.skip_separators();
            if self.peek_keyword() == Some("esac") {
                break;
            }
            if self.at_end() {
                return Err(ParseError::Incomplete);
            }

            // Patterns: an optional `(`, then word ( `|` word )* `)`.
            if matches!(self.peek(), Some(Token::LParen)) {
                self.pos += 1;
            }
            let mut patterns = vec![self.expect_word_token()?];
            while matches!(self.peek(), Some(Token::Pipe)) {
                self.pos += 1;
                patterns.push(self.expect_word_token()?);
            }
            match self.peek() {
                Some(Token::RParen) => self.pos += 1,
                None => return Err(ParseError::Incomplete),
                _ => return Err(ParseError::Syntax("expected `)` in case".into())),
            }

            let body = self.parse_list()?;
            if matches!(self.peek(), Some(Token::DSemi)) {
                self.pos += 1;
            }
            items.push((patterns, body));
        }

        self.expect_keyword("esac")?;
        Ok(RawCommand::Compound(Box::new(Compound::Case {
            word,
            items,
        })))
    }

    /// Consume the current token, requiring it to be a `Word`.
    fn expect_word_token(&mut self) -> Result<Word, ParseError> {
        match self.peek() {
            Some(Token::Word(_)) => {
                let Some(Token::Word(w)) = self.advance() else {
                    unreachable!()
                };
                Ok(w)
            }
            None => Err(ParseError::Incomplete),
            _ => Err(ParseError::Syntax("expected a word".into())),
        }
    }

    fn expect_keyword(&mut self, kw: &str) -> Result<(), ParseError> {
        self.skip_newlines();
        match self.peek_keyword() {
            Some(found) if found == kw => {
                self.pos += 1;
                Ok(())
            }
            _ => Err(self.eof_or_syntax(&format!("expected `{kw}`"))),
        }
    }

    fn expect_word(&mut self, after: &str) -> Result<Word, ParseError> {
        match self.advance() {
            Some(Token::Word(w)) => Ok(w),
            None => Err(ParseError::Incomplete),
            _ => Err(ParseError::Syntax(format!(
                "expected filename after `{after}`"
            ))),
        }
    }

    /// A bare identifier word, e.g. the variable of a `for` loop.
    fn expect_name(&mut self) -> Result<String, ParseError> {
        match self.peek() {
            Some(Token::Word(parts)) => {
                if let [WordPart::Unquoted(s)] = parts.as_slice()
                    && is_name(s)
                {
                    let name = s.clone();
                    self.pos += 1;
                    return Ok(name);
                }
                Err(ParseError::Syntax("expected a variable name".into()))
            }
            None => Err(ParseError::Incomplete),
            _ => Err(ParseError::Syntax("expected a variable name".into())),
        }
    }

    /// Pick `Incomplete` (more input may finish it) vs a hard syntax error.
    fn eof_or_syntax(&self, msg: &str) -> ParseError {
        if self.at_end() {
            ParseError::Incomplete
        } else {
            ParseError::Syntax(msg.to_string())
        }
    }
}

/// The reserved word a token represents, if it's a single unquoted keyword.
fn as_keyword(tok: &Token) -> Option<&'static str> {
    if let Token::Word(parts) = tok
        && let [WordPart::Unquoted(s)] = parts.as_slice()
    {
        return RESERVED.iter().copied().find(|&kw| kw == s);
    }
    None
}

/// Reserved words that begin a command (vs. ones that close a construct).
fn is_command_start(kw: &str) -> bool {
    matches!(kw, "if" | "while" | "until" | "for" | "case" | "{")
}

fn is_name(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c == '_' || c.is_ascii_alphabetic())
        && chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

fn describe(tok: &Token) -> String {
    match tok {
        Token::Word(_) => "word".into(),
        Token::Pipe => "|".into(),
        Token::Or => "||".into(),
        Token::And => "&&".into(),
        Token::Amp => "&".into(),
        Token::Semi => ";".into(),
        Token::DSemi => ";;".into(),
        Token::LParen => "(".into(),
        Token::RParen => ")".into(),
        Token::Redirect(_) => "redirection".into(),
        Token::Newline => "newline".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(word: &Word) -> String {
        word.iter()
            .map(|p| match p {
                WordPart::Literal(s) | WordPart::Unquoted(s) | WordPart::Quoted(s) => s.as_str(),
            })
            .collect()
    }

    /// The lone job's and-or list.
    fn only(list: &CommandList) -> &AndOrList {
        assert_eq!(list.jobs.len(), 1);
        &list.jobs[0].list
    }

    /// Extract a simple command's argv as strings.
    fn argv_text(cmd: &RawCommand) -> Vec<String> {
        match cmd {
            RawCommand::Simple(s) => s.argv.iter().map(text).collect(),
            RawCommand::Compound(_) => panic!("expected a simple command"),
        }
    }

    fn first_cmd(list: &CommandList) -> &RawCommand {
        &only(list).first.commands[0]
    }

    fn parse_ok(input: &str) -> CommandList {
        parse(input).unwrap()
    }

    #[test]
    fn single_command() {
        let p = parse_ok("ls -la");
        assert_eq!(argv_text(first_cmd(&p)), vec!["ls", "-la"]);
    }

    #[test]
    fn pipeline_splits() {
        let p = parse_ok("ls | grep rs | wc -l");
        assert_eq!(only(&p).first.commands.len(), 3);
    }

    #[test]
    fn captures_redirects() {
        let p = parse_ok("sort < in.txt >> out.txt");
        match first_cmd(&p) {
            RawCommand::Simple(s) => assert_eq!(s.redirects.len(), 2),
            _ => panic!(),
        }
    }

    #[test]
    fn fd_redirects_parse() {
        let p = parse_ok("cmd 2> err > out");
        match first_cmd(&p) {
            RawCommand::Simple(s) => {
                assert_eq!(s.redirects.len(), 2);
                assert!(matches!(s.redirects[0], RawRedirect::File { fd: 2, .. }));
                assert!(matches!(s.redirects[1], RawRedirect::File { fd: 1, .. }));
            }
            _ => panic!(),
        }
        match first_cmd(&parse_ok("cmd > f 2>&1")) {
            RawCommand::Simple(s) => {
                assert!(matches!(
                    s.redirects[1],
                    RawRedirect::Dup { fd: 2, target: 1 }
                ));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn dangling_pipe_is_incomplete() {
        assert!(matches!(parse("ls |"), Err(ParseError::Incomplete)));
        assert!(parse("| ls").is_err());
    }

    #[test]
    fn parses_and_or() {
        let p = parse_ok("a && b | c || d");
        let a = only(&p);
        let connectors: Vec<Connector> = a.rest.iter().map(|(c, _)| *c).collect();
        assert_eq!(connectors, vec![Connector::And, Connector::Or]);
        assert_eq!(a.rest[0].1.commands.len(), 2);
    }

    #[test]
    fn semicolon_and_background_separate_jobs() {
        let p = parse_ok("a ; b ; c");
        assert_eq!(p.jobs.len(), 3);
        let p = parse_ok("sleep 1 & echo done");
        assert!(p.jobs[0].background);
        assert!(!p.jobs[1].background);
    }

    #[test]
    fn newline_separates_jobs() {
        let p = parse_ok("a\nb\nc");
        assert_eq!(p.jobs.len(), 3);
        // Blank lines collapse.
        assert_eq!(parse_ok("a\n\n\nb").jobs.len(), 2);
    }

    #[test]
    fn comment_only_is_a_noop() {
        assert!(parse_ok("# just a comment").jobs.is_empty());
        assert!(parse_ok("   ").jobs.is_empty());
        assert_eq!(parse_ok("ls -l  # list").jobs.len(), 1);
    }

    #[test]
    fn if_then_else() {
        let p = parse_ok("if true; then echo yes; else echo no; fi");
        match first_cmd(&p) {
            RawCommand::Compound(c) => match c.as_ref() {
                Compound::If {
                    branches,
                    else_body,
                } => {
                    assert_eq!(branches.len(), 1);
                    assert!(else_body.is_some());
                }
                _ => panic!(),
            },
            _ => panic!("expected compound"),
        }
    }

    #[test]
    fn if_elif_chain() {
        let p = parse_ok("if a; then b; elif c; then d; elif e; then f; fi");
        match first_cmd(&p) {
            RawCommand::Compound(c) => match c.as_ref() {
                Compound::If {
                    branches,
                    else_body,
                } => {
                    assert_eq!(branches.len(), 3);
                    assert!(else_body.is_none());
                }
                _ => panic!(),
            },
            _ => panic!(),
        }
    }

    #[test]
    fn while_and_for() {
        assert!(matches!(
            first_cmd(&parse_ok("while true; do echo x; done")),
            RawCommand::Compound(_)
        ));
        let p = parse_ok("for x in a b c; do echo $x; done");
        match first_cmd(&p) {
            RawCommand::Compound(c) => match c.as_ref() {
                Compound::For { var, words, .. } => {
                    assert_eq!(var, "x");
                    assert_eq!(words.len(), 3);
                }
                _ => panic!(),
            },
            _ => panic!(),
        }
    }

    #[test]
    fn case_clause() {
        let p = parse_ok("case $x in a) echo A ;; b|c) echo BC ;; *) echo other ;; esac");
        match first_cmd(&p) {
            RawCommand::Compound(c) => match c.as_ref() {
                Compound::Case { items, .. } => {
                    assert_eq!(items.len(), 3);
                    assert_eq!(items[1].0.len(), 2); // b|c → two patterns
                }
                _ => panic!(),
            },
            _ => panic!("expected compound"),
        }
        // Multi-line and an empty body are both fine.
        assert!(matches!(
            first_cmd(&parse_ok("case x in\n  y) ;;\n  *) echo z ;;\nesac")),
            RawCommand::Compound(_)
        ));
    }

    #[test]
    fn subshell() {
        match first_cmd(&parse_ok("(cd x; ls)")) {
            RawCommand::Compound(c) => assert!(matches!(c.as_ref(), Compound::Subshell(_))),
            _ => panic!("expected subshell"),
        }
        assert!(matches!(parse("(echo hi"), Err(ParseError::Incomplete)));
    }

    #[test]
    fn function_and_group() {
        match first_cmd(&parse_ok("greet() { echo hi; }")) {
            RawCommand::Compound(c) => match c.as_ref() {
                Compound::FuncDef { name, .. } => assert_eq!(name, "greet"),
                _ => panic!("expected funcdef"),
            },
            _ => panic!("expected compound"),
        }
        assert!(matches!(
            first_cmd(&parse_ok("{ echo a; echo b; }")),
            RawCommand::Compound(_)
        ));
        // `name` is a plain word when not followed by `()`.
        assert_eq!(
            argv_text(first_cmd(&parse_ok("greet hi"))),
            vec!["greet", "hi"]
        );
    }

    #[test]
    fn multiline_if_across_newlines() {
        let p = parse_ok("if true\nthen\n  echo hi\nfi");
        assert!(matches!(first_cmd(&p), RawCommand::Compound(_)));
    }

    #[test]
    fn incomplete_compound_reports_incomplete() {
        assert!(matches!(
            parse("if true; then echo hi"),
            Err(ParseError::Incomplete)
        ));
        assert!(matches!(
            parse("while true; do"),
            Err(ParseError::Incomplete)
        ));
        assert!(matches!(parse("for x in a b"), Err(ParseError::Incomplete)));
    }

    #[test]
    fn reserved_word_as_argument() {
        // After the command word, `if`/`then` are ordinary arguments.
        let p = parse_ok("echo if then fi");
        assert_eq!(argv_text(first_cmd(&p)), vec!["echo", "if", "then", "fi"]);
    }

    #[test]
    fn stray_terminator_is_error() {
        assert!(matches!(parse("fi"), Err(ParseError::Syntax(_))));
        assert!(matches!(parse("then echo"), Err(ParseError::Syntax(_))));
    }
}

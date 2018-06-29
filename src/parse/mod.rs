//! SMT-LIB 2 horn clause problem parser.

use common::* ;
use instance::{
  *, info::VarInfo
} ;

mod ptterms ;
pub use self::ptterms::* ;

use consts::keywords ;



/// Result yielded by the parser.
#[derive(PartialEq, Eq)]
pub enum Parsed {
  /// Check-sat.
  CheckSat,
  /// Get-model.
  GetModel,
  /// Get unsat core.
  GetUnsatCore,
  /// Get unsat proof.
  GetProof,
  /// Exit.
  Exit,
  /// Only parsed some item(s), no query.
  Items,
  /// Reset.
  Reset,
  /// End of file.
  Eof,
}



lazy_static!{
  /// Set of legal special characters in identifiers.
  static ref id_special_chars: HashSet<& 'static str> = {
    let mut set = HashSet::with_capacity(17) ;
    set.insert("~") ;
    set.insert("!") ;
    set.insert("@") ;
    set.insert("$") ;
    set.insert("%") ;
    set.insert("^") ;
    set.insert("&") ;
    set.insert("*") ;
    set.insert("_") ;
    set.insert("-") ;
    set.insert("+") ;
    set.insert("=") ;
    set.insert("<") ;
    set.insert(">") ;
    set.insert(".") ;
    set.insert("?") ;
    set.insert("/") ;
    set
  } ;
}



/// String extensions, lift char functions.
pub trait StringExt {
  /// Lifts `char::is_alphanumeric`.
  fn is_alphanumeric(& self) -> bool ;
  /// Lifts `char::is_alphabetic`.
  fn is_alphabetic(& self) -> bool ;
  /// Lifts `char::is_numeric`.
  fn is_numeric(& self) -> bool ;
}
impl StringExt for str {
  fn is_alphanumeric(& self) -> bool {
    for char in self.chars() {
      if ! char.is_alphanumeric() { return false }
    }
    true
  }
  fn is_alphabetic(& self) -> bool {
    for char in self.chars() {
      if ! char.is_alphabetic() { return false }
    }
    true
  }
  fn is_numeric(& self) -> bool {
    for char in self.chars() {
      if ! char.is_numeric() { return false }
    }
    true
  }
}



/// Extends `BufRead` with SMT-LIB 2 item parsing.
pub trait ItemRead {
  /// Reads the next item.
  ///
  /// - returns the **number of lines** read, not the number of bytes read
  /// - returns `None` once it finds `eof` and no item prior
  fn read_item(& mut self, buf: & mut String) -> Res<usize> ;
}
impl<T: ::std::io::BufRead> ItemRead for T {
  fn read_item(& mut self, buf: & mut String) -> Res<usize> {
    let mut line_count = 0 ;
    let mut start = buf.len() ;
    let mut char_override: Option<char> = None ;
    let mut opn_parens = 0 ;
    let mut cls_parens = 0 ;

    fn search_char(
      char: char, chars: & mut ::std::str::Chars
    ) -> Option<char> {
      for c in chars {
        if char == c {
          return None
        }
      }
      Some(char)
    }

    'read_lines: while self.read_line( buf ) ? != 0 {
      line_count += 1 ;
      debug_assert!( opn_parens >= cls_parens ) ;
      let mut chars = buf[start..].chars() ;
      
      if let Some(char) = char_override {
        char_override = search_char(char, & mut chars)
      }

      'inspect_chars: while let Some(c) = chars.next() {
        match c {
          '(' => opn_parens += 1,
          ')' => cls_parens += 1,
          '|' => {
            debug_assert!( char_override.is_none() ) ;
            char_override = search_char('|', & mut chars)
          },
          '"' => {
            debug_assert!( char_override.is_none() ) ;
            char_override = search_char('"', & mut chars)
          },
          ';' => break 'inspect_chars,
          _ => (),
        }
      }

      if opn_parens > 0 && opn_parens == cls_parens
      || opn_parens < cls_parens {
        // Something's wrong, let the parser handle it.
        break 'read_lines
      }

      start = buf.len()
    }

    Ok(line_count)
  }
}


/// String cursor.
pub type Cursor = usize ;
/// Position in the text.
#[derive(
  Clone, Copy, Debug,
  PartialEq, Eq, Hash,
  PartialOrd, Ord
)]
pub struct Pos(usize) ;
impl ::std::ops::Deref for Pos {
  type Target = usize ;
  fn deref(& self) -> & usize { & self.0 }
}




/// Result of parsing a clause.
enum ClauseRes {
  /// Clause parsed, but it was redundant.
  Skipped,
  /// Clause parsed and added.
  Added(ClsIdx),
}
impl ClauseRes {
  /// Turns itself in an option.
  pub fn into_option(self) -> Option<ClsIdx> {
    if let ClauseRes::Added(i) = self {
      Some(i)
    } else {
      None
    }
  }
}



/// Term stack frame used in the parser to avoid recursion.
struct TermFrame {
  /// Operator when going up.
  op: Op,
  /// Position of the operator.
  op_pos: Pos,
  /// Position of the arguments.
  args_pos: Vec<Pos>,
  /// Arguments.
  args: Vec<Term>,
  /// Let-binding count.
  let_count: LetCount,
}
impl TermFrame {
  /// Constructor.
  pub fn new(
    op: Op, op_pos: Pos, let_count: LetCount
  ) -> Self {
    TermFrame {
      op, op_pos, let_count,
      args_pos: Vec::with_capacity(11),
      args: Vec::with_capacity(11),
    }
  }

  /// Pushes an argument.
  pub fn push_arg(& mut self, pos: Pos, arg: Term) {
    debug_assert_eq! { self.args_pos.len(), self.args.len() }
    self.args_pos.push(pos) ;
    self.args.push(arg)
  }

  /// True if the frame has no arguments.
  pub fn is_empty(& self) -> bool {
    debug_assert_eq! { self.args_pos.len(), self.args.len() }
    self.args_pos.is_empty()
  }

  /// Retrieves the let-binding count and sets the internal one to zero.
  pub fn let_count(& mut self) -> LetCount {
    ::std::mem::replace( & mut self.let_count, 0.into() )
  }

  /// Destroys the frame.
  pub fn destroy(self) -> (
    Op, Pos, Vec<Pos>, Vec<Term>
  ) {
    (self.op, self.op_pos, self.args_pos, self.args)
  }
}


/// Parser context.
#[derive(Default)]
pub struct ParserCxt {
  /// Term stack to avoid recursion.
  term_stack: Vec<TermFrame>,
  /// Memory for backtracking.
  mem: Vec<Cursor>,
  /// Map from predicate names to predicate indices.
  pred_name_map: HashMap<String, PrdIdx>,
}
impl ParserCxt {
  /// Constructor.
  pub fn new() -> Self {
    ParserCxt {
      term_stack: Vec::with_capacity(17),
      mem: Vec::with_capacity(17),
      pred_name_map: HashMap::with_capacity(42),
    }
  }
  /// Generates a parser from itself.
  pub fn parser<'cxt, 's>(
    & 'cxt mut self, string: & 's str, line_off: usize,
    _profiler: & 'cxt Profiler
  ) -> Parser<'cxt, 's> {
    debug_assert!( self.mem.is_empty() ) ;
    Parser {
      cxt: self,
      string,
      cursor: 0,
      line_off,
      bindings: Vec::with_capacity(7),
      _profiler,
    }
  }

  /// Resets the parser.
  pub fn reset(& mut self) {
    self.pred_name_map.clear()
  }
}


/// Wraps an integer, represents a number of let-bindings parsed.
#[must_use]
#[derive(Clone, Copy)]
struct LetCount {
  n: usize
}
impl LetCount {
  /// True if zero.
  pub fn is_zero(self) -> bool{ self.n == 0 }
}
impl From<usize> for LetCount {
  fn from(n: usize) -> Self { LetCount { n } }
}


/// Parser structure. Generated from a `ParserCxt`.
pub struct Parser<'cxt, 's> {
  /// Parsing context.
  cxt: & 'cxt mut ParserCxt,
  /// Text being read (for errors).
  string: & 's str,
  /// Current position in the text.
  cursor: Cursor,
  /// Line offset (for errors).
  line_off: usize,
  /// Stack of bindings.
  bindings: Vec< HashMap<& 's str, PTTerms> >,
  /// Profiler.
  _profiler: & 'cxt Profiler,
}


impl<'cxt, 's> Parser<'cxt, 's> {


  /// Returns the text that hasn't been parsed yet.
  pub fn rest(& self) -> & str {
    & self.string[self.cursor..]
  }

  /// Generates a parse error at the current position.
  fn error_here<S: Into<String>>(& mut self, msg: S) -> ErrorKind {
    let pos = self.pos() ;
    self.error(pos, msg)
  }

  /// Generates a parse error at the given position.
  fn error<S: Into<String>>(
    & self, char_pos: Pos, msg: S
  ) -> ErrorKind {
    let mut char_pos = * char_pos ;
    let msg = msg.into() ;
    let mut line_count = self.line_off ;
    let (mut pref, mut token, mut suff) = (
      "".to_string(), "<eof>".to_string(), "".to_string()
    ) ;
    for line in self.string.lines() {
      line_count += 1 ;
      if char_pos < line.len() {
        pref = line[0..char_pos].to_string() ;
        token = line[char_pos..(char_pos + 1)].to_string() ;
        suff = line[(char_pos + 1)..line.len()].to_string() ;
        break
      } else if char_pos == line.len() {
        pref = line.into() ;
        token = "\\n".into() ;
        suff = "".into() ;
        break
      } else {
        char_pos -= line.len() + 1
      }
    }
    ErrorKind::ParseError(
      ParseErrorData {
        msg, pref, token, suff, line: Some(line_count)
      }
    )
  }


  /// Returns `true` if there's still things to parse.
  #[inline]
  fn has_next(& self) -> bool {
    self.cursor < self.string.len()
  }
  /// The next character, does not move the cursor.
  fn peek(& self) -> Option<& 's str> {
    if self.has_next() {
      Some(
        & self.string[ self.cursor .. self.cursor + 1 ]
      )
    } else {
      None
    }
  }


  /// True if the current character is a legal unquoted identifier character.
  fn legal_id_char(& self) -> bool {
    if self.cursor >= self.string.len() {
      false
    } else {
      let char = & self.string[ self.cursor .. self.cursor + 1 ] ;
      char.is_alphanumeric()
      || id_special_chars.contains(& char)
    }
  }

  /// The next character.
  fn next(& mut self) -> Option<& 's str> {
    if self.has_next() {
      self.cursor += 1 ;
      Some(
        & self.string[ self.cursor - 1 .. self.cursor ]
      )
    } else {
      None
    }
  }
  /// Moves the cursor back by `n` character.
  ///
  /// # Panic
  ///
  /// - if `self.cursor < n`
  fn move_back(& mut self, n: usize) {
    debug_assert! { self.cursor >= n }
    self.cursor -= n
  }

  /// Backtracks to a precise position.
  pub fn backtrack_to(& mut self, Pos(pos): Pos) {
    self.cursor = pos
  }

  /// Returns the current position.
  pub fn pos(& mut self) -> Pos {
    Pos( self.cursor )
  }

  /// Consumes whitespaces and comments.
  pub fn ws_cmt(& mut self) {
    let mut done = false ;
    while ! done {
      // Eat spaces.
      let rest = & self.string[ self.cursor .. ] ;
      let trimmed = rest.trim_left() ;
      let diff = rest.len() - trimmed.len() ;
      done = diff == 0 ;
      self.cursor += diff ;

      // Eat comments.
      match self.next() {
        Some(";") => {
          done = false ;
          'eat_line: while let Some(char) = self.next() {
            if char == "\n" || char == "\r" {
              break 'eat_line
            }
          }
        },
        Some(_) => self.move_back(1),
        None => (),
      }
    }
  }

  /// Parses a string or fails.
  pub fn tag(& mut self, tag: & str) -> Res<()> {
    if self.tag_opt(tag) {
      Ok(())
    } else {
      bail!(
        self.error_here(
          format!("expected `{}`", conf.emph(tag))
        )
      )
    }
  }
  /// Parses a string or fails with error customization.
  fn tag_err<S>(& mut self, tag: & str, err: S) -> Res<()>
  where S: Into<String> {
    if self.tag_opt(tag) {
      Ok(())
    } else {
      bail!(
        self.error_here( err.into().to_string() )
      )
    }
  }
  /// Tries parsing a string.
  pub fn tag_opt(& mut self, tag: & str) -> bool {
    self.tag_opt_pos(tag).is_some()
  }
  /// Tries parsing a string. Returns the position of the start of the tag.
  fn tag_opt_pos(& mut self, tag: & str) -> Option<Pos> {
    if self.string.len() < self.cursor + tag.len() {
      None
    } else if & self.string[
      self.cursor .. self.cursor + tag.len()
    ] == tag {
      let res = Some(self.pos()) ;
      self.cursor += tag.len() ;
      res
    } else {
      None
    }
  }

  /// Parses an ident of fails.
  pub fn ident(& mut self) -> Res< (Pos, & 's str) > {
    if let Some(id) = self.ident_opt() ? {
      Ok(id)
    } else {
      bail!(
        self.error_here("expected an identifier")
      )
    }
  }
  /// Tries to parse an ident.
  pub fn ident_opt(& mut self) -> Res< Option< (Pos, & 's str) > > {
    let ident_start_pos = self.pos() ;
    if let Some(id) = self.unsafe_ident_opt() ? {
      if keywords::is_keyword(id) {
        bail!(
          self.error(
            ident_start_pos,
            format!(
              "illegal usage of keyword `{}`",
              conf.bad(id)
            )
          )
        )
      } else {
        Ok( Some((ident_start_pos, id)) )
      }
    } else {
      Ok(None)
    }
  }
  /// Tries to parse an ident, does not check anything about the ident.
  fn unsafe_ident_opt(& mut self) -> Res< Option<& 's str> > {
    let ident_start_pos = self.pos() ;
    if let Some(char) = self.next() {
      if char == "|" {
        let (mut legal_unquoted, mut is_first) = (true, true) ;
        while let Some(char) = self.next() {
          if char == "|" {
            return Ok(
              Some(
                if legal_unquoted {
                  & self.string[ * ident_start_pos + 1 .. self.cursor - 1 ]
                } else {
                  & self.string[ * ident_start_pos .. self.cursor ]
                }
              )
            )
          } else {
            legal_unquoted = legal_unquoted && (
              ( ! is_first && char.is_alphanumeric() ) ||
              (   is_first && char.is_alphabetic()   ) ||
              id_special_chars.contains(char)
            ) ;
            is_first = false ;
          }
        }
        bail!(
          self.error(
            ident_start_pos,
            "expected `|` closing this quoted identifier, \
            found eof".to_string()
          )
        )
      } else if char.is_alphabetic() || id_special_chars.contains(& char) {
        while let Some(char) = self.next() {
          if ! (
            char.is_alphanumeric() || id_special_chars.contains(& char)
          ) {
            self.move_back(1) ;
            break
          }
        }
        Ok(
          Some(
            & self.string[ * ident_start_pos .. self.cursor ]
          )
        )
      } else {
        self.backtrack_to(ident_start_pos) ;
        Ok(None)
      }
    } else {
      Ok(None)
    }
  }

  /// Consumes characters until some character.
  ///
  /// Returns `true` iff `char` was found. Hence, returns `false` iff `eof` was
  /// reached.
  fn eat_until(& mut self, char: char, inclusive: bool) -> bool {
    for c in self.string[ self.cursor .. ].chars() {
      if char == c {
        if inclusive {
          self.cursor += 1
        }
        return true
      } else {
        self.cursor += 1
      }
    }
    false
  }

  /// Returns all the characters until some character.
  ///
  /// `None` iff `char` was not found, i.e. `eat_until` returns `false`.
  fn get_until(& mut self, char: char, inclusive: bool) -> Option<& 's str> {
    let start_pos = self.pos() ;
    let found_id = self.eat_until(char, inclusive) ;
    if found_id {
      Some( & self.string[ * start_pos .. self.cursor ] )
    } else {
      None
    }
  }

  /// Parses a set-info.
  fn set_info(& mut self) -> Res<bool> {
    if ! self.tag_opt("set-info") {
      return Ok(false)
    }
    self.ws_cmt() ;
    self.tag(":") ? ;
    self.ws_cmt() ;
    let _ = self.ident() ? ;
    self.ws_cmt() ;
    if self.tag_opt("\"") {
      let found_it = self.eat_until('"', true) ;
      if ! found_it {
        bail!(
          self.error_here("expected closing `\"`, found <eof>")
        )
      }
    } else if self.ident_opt()?.is_some() {
      ()
    }
    Ok(true)
  }

  /// Set-option.
  fn set_option(& mut self) -> Res< Option<(& 's str, & 's str)> > {
    let start_pos = self.pos() ;
    if ! self.tag_opt("set-option") {
      return Ok(None)
    }
    self.ws_cmt() ;
    self.tag(":") ? ;
    let key = self.ident()?.1 ;
    self.ws_cmt() ;
    let val = if self.tag_opt("|") {
      if let Some(res) = self.get_until('|', true) {
        res
      } else {
        bail!(
          self.error_here("could not find closing `|` opened")
        )
      }
    } else if self.tag_opt("\"") {
      if let Some(res) = self.get_until('"', true) {
        res
      } else {
        bail!(
          self.error_here("could not find closing `\"` opened")
        )
      }
    } else if let Some(res) = self.get_until(')', false) {
      res.trim()
    } else {
      self.backtrack_to(start_pos) ;
      bail!(
        self.error_here("could not find closing `)` for this set-option")
      )
    } ;
    Ok(Some((key, val)))
  }

  /// Parses an echo.
  fn echo(& mut self) -> Res< Option<& 's str> > {
    if ! self.tag_opt("echo") {
      return Ok(None)
    }
    self.ws_cmt() ;
    self.tag("\"") ? ;
    let blah = self.get_until('"', false) ;
    self.tag("\"") ? ;
    if let Some(blah) = blah {
      Ok( Some(blah) )
    } else {
      bail!(
        self.error_here("expected closing `\"`, found <eof>")
      )
    }
  }

  /// Parses a set-logic.
  fn set_logic(& mut self) -> Res<bool> {
    if ! self.tag_opt("set-logic") {
      return Ok(false)
    }

    self.ws_cmt() ;
    if ! self.tag_opt("HORN") {
      bail!( self.error_here("unknown logic: ") )
    }
    Ok(true)
  }

  /// Parses a sort or fails.
  fn sort(& mut self) -> Res<Typ> {
    if let Some(sort) = self.sort_opt() ? {
      Ok(sort)
    } else {
      bail!( self.error_here("expected sort (Int or Bool)") )
    }
  }




  /// Tries to parse a sort.
  pub fn sort_opt(& mut self) -> Res<Option<Typ>> {
    // Compound type under construction.
    //
    // The position is always that of the opening paren of the type.
    enum CTyp<'a> {
      // Array under construction, meaning we're parsing the index sort.
      Array { pos: Pos },
      // Array with a source, meaning we're parsing the target sort.
      ArraySrc { pos: Pos, src: Typ },
      // A datatype application.
      DTyp { name: & 'a str, pos: Pos, typs: dtyp::TPrmMap<Typ> }
    }

    let mut stack = vec![] ;

    let start_pos = self.pos() ;

    'go_down: loop {
      self.ws_cmt() ;
      let current_pos = self.pos() ;

      let mut typ = if self.tag_opt("(") {
        self.ws_cmt() ;
        // Parsing a compound type.

        if self.tag_opt("Array") {
          if ! self.legal_id_char() {
            // We're parsing an array type.
            stack.push( CTyp::Array { pos: current_pos } ) ;
            continue 'go_down
          } else {
            None
          }
        } else if let Some((pos, name)) = self.ident_opt() ? {
          stack.push(
            CTyp::DTyp { name, pos, typs: dtyp::TPrmMap::new() }
          ) ;
          continue 'go_down
        } else {
          None
        }

      } else if self.tag_opt("Int") {
        if ! self.legal_id_char() {
          Some(typ::int())
        } else {
          None
        }
      } else if self.tag_opt("Real") {
        if ! self.legal_id_char() {
          Some(typ::real())
        } else {
          None
        }
      } else if self.tag_opt("Bool") {
        if ! self.legal_id_char() {
          Some(typ::bool())
        } else {
          None
        }
      } else {
        None
      } ;

      if typ.is_none() {
        if let Some((pos, name)) = self.ident_opt() ? {
          if let Ok(dtyp) = dtyp::get(name) {
            typ = Some(
              typ::dtyp( dtyp, vec![].into() )
            )
          } else {
            bail!(
              self.error(
                pos, format!("unknown sort `{}`", conf.bad(name))
              )
            )
          }
        }
      }

      'go_up: loop {

        match stack.pop() {

          Some(CTyp::Array { pos }) => if let Some(src) = typ {
            stack.push( CTyp::ArraySrc { pos, src } ) ;
            // Need to parse the domain now.
            continue 'go_down
          } else {
            Err::<_, Error>(
              self.error(pos, "while parsing this array sort").into()
            ).chain_err(
              || self.error(current_pos, "expected index sort")
            ) ?
          },

          Some(CTyp::ArraySrc { pos, src }) => if let Some(tgt) = typ {
            typ = Some( typ::array(src, tgt) ) ;

            // Parse closing paren.
            self.ws_cmt() ;
            if ! self.tag_opt(")") {
              Err::<_, Error>(
                self.error(pos, "while parsing this array sort").into()
              ).chain_err(
                || self.error(
                  current_pos, "expected expected closing paren"
                )
              ) ?
            }

            continue 'go_up
          } else {
            Err::<_, Error>(
              self.error(pos, "while parsing this array sort").into()
            ).chain_err(
              || self.error(current_pos, "expected domain sort")
            ) ?
          },

          Some(CTyp::DTyp { name, pos, mut typs }) => if let Some(prm) = typ {
            typs.push(prm) ;

            self.ws_cmt() ;
            if self.tag_opt(")") {
              if let Ok(dtyp) = dtyp::get(name) {
                typ = Some( typ::dtyp(dtyp, typs) )
              } else {
                bail!(
                  self.error(
                    pos, format!(
                      "unknown sort `{}`", conf.bad(name)
                    )
                  )
                )
              }
              continue 'go_up
            } else {
              stack.push( CTyp::DTyp { name, pos, typs } ) ;
              continue 'go_down
            }
          } else {
            Err::<_, Error>(
              self.error(pos, "while parsing this sort").into()
            ) ?
          },

          None => if typ.is_none() {
            self.backtrack_to(start_pos) ;
            return Ok(None)
          } else {
            return Ok(typ)
          }
        }

      }
    }

  }



  /// Parses a sort.
  pub fn nu_sort(
    & mut self, type_params: & BTreeMap<& 's str, dtyp::TPrmIdx>
  ) -> Res<dtyp::PartialTyp> {
    if let Some(res) = self.nu_sort_opt(type_params) ? {
      Ok(res)
    } else {
      bail!( self.error_here("expected sort") )
    }
  }



  /// Tries to parse a sort.
  pub fn nu_sort_opt(
    & mut self, type_params: & BTreeMap<& 's str, dtyp::TPrmIdx>
  ) -> Res<Option<dtyp::PartialTyp>> {
    use dtyp::PartialTyp ;

    // Compound type under construction.
    //
    // The position is always that of the opening paren of the type.
    enum CTyp<'a> {
      // Array under construction, meaning we're parsing the index sort.
      Array { pos: Pos },
      // Array with a source, meaning we're parsing the target sort.
      ArraySrc { pos: Pos, src: PartialTyp },
      // Datatype storing the name, the position of the name, and the types.
      DTyp { name: & 'a str, pos: Pos, typs: dtyp::TPrmMap< PartialTyp > },
    }

    let mut stack = vec![] ;

    let start_pos = self.pos() ;

    'go_down: loop {
      self.ws_cmt() ;
      let current_pos = self.pos() ;

      let mut typ = if self.tag_opt("(") {
        self.ws_cmt() ;
        // Parsing a compound type.

        if self.tag_opt("Array") {
          if ! self.legal_id_char() {
            // We're parsing an array type.
            stack.push( CTyp::Array { pos: current_pos } ) ;
            continue 'go_down
          } else {
            None
          }
        } else if let Some((pos, name)) = self.ident_opt() ? {
          stack.push(
            CTyp::DTyp { name, pos, typs: dtyp::TPrmMap::new() }
          ) ;
          continue 'go_down
        } else {
          None
        }

      } else if self.tag_opt("Int") {
        if ! self.legal_id_char() {
          Some( typ::int().into() )
        } else {
          None
        }

      } else if self.tag_opt("Real") {
        if ! self.legal_id_char() {
          Some( typ::real().into() )
        } else {
          None
        }

      } else if self.tag_opt("Bool") {
        if ! self.legal_id_char() {
          Some( typ::bool().into() )
        } else {
          None
        }

      } else {
        None
      } ;

      if typ.is_none() {
        if let Some((pos, name)) = self.ident_opt() ? {
          // Type parameter?
          typ = if let Some(idx) = type_params.get(name) {
            Some( PartialTyp::Param(* idx) )
          } else {
            Some(
              PartialTyp::DTyp( name.into(), pos, vec![].into() )
            )
          }

        }
      }

      'go_up: loop {

        match stack.pop() {

          Some(CTyp::Array { pos }) => if let Some(src) = typ {
            stack.push( CTyp::ArraySrc { pos, src } ) ;
            // Need to parse the domain now.
            continue 'go_down
          } else {
            Err::<_, Error>(
              self.error(pos, "while parsing this array sort").into()
            ).chain_err(
              || self.error(current_pos, "expected index sort")
            ) ?
          },

          Some(CTyp::ArraySrc { pos, src }) => if let Some(tgt) = typ {
            typ = Some(
              PartialTyp::Array( Box::new(src), Box::new(tgt) )
            ) ;

            // Parse closing paren.
            self.ws_cmt() ;
            if ! self.tag_opt(")") {
              Err::<_, Error>(
                self.error(pos, "while parsing this array sort").into()
              ).chain_err(
                || self.error(
                  current_pos, "expected expected closing paren"
                )
              ) ?
            }

            continue 'go_up
          } else {
            Err::<_, Error>(
              self.error(pos, "while parsing this array sort").into()
            ).chain_err(
              || self.error(current_pos, "expected domain sort")
            ) ?
          },

          Some(CTyp::DTyp { name, pos, mut typs }) => if let Some(t) = typ {
            typs.push(t) ;
            self.ws_cmt() ;
            if self.tag_opt(")") {
              typ = Some( PartialTyp::DTyp(name.into(), pos, typs) ) ;
              continue 'go_up
            } else {
              stack.push( CTyp::DTyp { name, pos, typs } ) ;
              continue 'go_down
            }
          } else {
            Err::<_, Error>(
              self.error(pos, "while parsing this datatype sort").into()
            ).chain_err(
              || self.error(current_pos, "expected sort")
            ) ?
          },

          None => if typ.is_none() {
            self.backtrack_to(start_pos) ;
            return Ok(None)
          } else {
            return Ok(typ)
          }
        }

      }
    }

  }






  /// Datatype declaration.
  fn dtyp_dec(& mut self) -> Res<bool> {
    if ! self.tag_opt(keywords::cmd::dec_dtyp) {
      return Ok(false)
    }

    let mut dtyps: Vec<(Pos, dtyp::RDTyp)> = vec![] ;

    let mut params = dtyp::TPrmMap::new() ;

    self.ws_cmt() ;
    self.tag("(") ? ;
    self.ws_cmt() ;

    let mut params_map = BTreeMap::new() ;

    // Type parameters.
    while let Some((pos, ident)) = self.ident_opt() ? {
      let idx = params.next_index() ;
      if let Some(prev) = params_map.insert(ident, idx) {
        bail!(
          self.error(
            pos, format!(
              "type parameters #{} and #{} have the same name `{}`",
              idx, prev, ident
            )
          )
        )
      }
      params.push( ident.to_string() ) ;
      self.ws_cmt()
    }

    self.tag(")") ? ;
    self.ws_cmt() ;

    self.tag("(") ? ;
    self.ws_cmt() ;

    // Datatype declarations.
    while self.tag_opt("(") {
      self.ws_cmt() ;
      let (dtyp_pos, dtyp_ident) = self.ident() ? ;
      self.ws_cmt() ;

      let mut dtyp = dtyp::RDTyp::new( dtyp_ident, params.clone() ) ;

      // Constructors.
      'constructors: loop {

        let (
          constructor_pos, constructor_ident, selectors
        ) = if self.tag_opt("(") {
          self.ws_cmt() ;
          let (constructor_pos, constructor_ident) = self.ident() ? ;
          self.ws_cmt() ;

          let mut selectors = dtyp::CArgs::new() ;

          // Selectors.
          while self.tag_opt("(") {
            self.ws_cmt() ;
            let (selector_pos, selector_ident) = self.ident() ? ;
            self.ws_cmt() ;

            if selectors.iter().any(
              |(id, _)| id == selector_ident
            ) {
              let error: Error = self.error(
                selector_pos,
                format!("found the selector `{}` twice", selector_ident)
              ).into() ;
              bail!(
                error.chain_err(
                  || self.error(
                    dtyp_pos, "in this datatype declaration"
                  )
                )
              )
            }

            let ptyp = self.nu_sort(& params_map) ? ;
            selectors.push(
              ( selector_ident.to_string(), ptyp )
            ) ;

            self.ws_cmt() ;
            self.tag(")") ? ;
            self.ws_cmt()
          }

          self.tag(")") ? ;
          self.ws_cmt() ;

          (constructor_pos, constructor_ident, selectors)

        } else if let Some(
          (constructor_pos, constructor_ident)
        ) = self.ident_opt() ? {
          self.ws_cmt() ;
          (constructor_pos, constructor_ident, dtyp::CArgs::new())

        } else {
          break 'constructors
        } ;


        dtyp.add_constructor(constructor_ident, selectors).chain_err(
          || self.error(constructor_pos, "in this constructor")
        ) ?

      }

      for (_, dt) in & mut dtyps {
        dt.add_dep( dtyp.name.clone() ) ;
        dtyp.add_dep( dt.name.clone() )
      }

      dtyps.push( (dtyp_pos, dtyp) ) ;

      self.tag_opt(")") ;
      self.ws_cmt()
    }

    self.tag(")") ? ;

    let mut final_dtyps = Vec::with_capacity( dtyps.len() ) ;

    for (dtyp_pos, dtyp) in dtyps {
      final_dtyps.push((
        dtyp_pos, dtyp::mk(dtyp).chain_err(
          || self.error(dtyp_pos, "while parsing this datatype constructor")
        ) ?
      ))
    }

    for (dtyp_pos, dtyp) in final_dtyps {
      if let Err((pos, err)) = dtyp.check() {
        let err: Error = self.error(pos, err).into() ;
        bail!(
          err.chain_err(
            || self.error(dtyp_pos, "in this datatype declaration")
          )
        )
      }
    }

    Ok(true)
  }



  /// Predicate declaration.
  fn pred_dec(& mut self, instance: & mut Instance) -> Res<bool> {
    if ! self.tag_opt(keywords::cmd::dec_fun) {
      return Ok(false)
    }

    self.ws_cmt() ;
    let (pos, ident) = self.ident() ? ;
    self.ws_cmt() ;
    self.tag("(") ? ;

    let mut sorts = Vec::with_capacity(11) ;
    self.ws_cmt() ;
    while let Some(ty) = self.sort_opt() ? {
      self.ws_cmt() ;
      sorts.push(ty) ;
    }
    sorts.shrink_to_fit() ;

    self.ws_cmt() ;
    self.tag(")") ? ;
    self.ws_cmt() ;
    if ! self.tag_opt("Bool") {
      bail!(
        self.error_here("expected Bool sort")
      )
    }

    let pred_index = instance.push_pred(
      ident.into(), VarMap::of(sorts)
    ) ;
    let prev = self.cxt.pred_name_map.insert(ident.into(), pred_index) ;
    if let Some(prev) = prev {
      bail!(
        self.error(
          pos,
          format!(
            "predicate `{}` is already declared",
            conf.bad( & format!("{}", instance[prev]) )
          )
        )
      )
    }

    Ok(true)
  }

  /// Parses some arguments `( (<id> <ty>) ... )`.
  fn args(
    & mut self,
    var_map: & mut VarInfos, hash_map: & mut HashMap<& 's str, VarIdx>
  ) -> Res<()> {
    self.tag("(") ? ;

    self.ws_cmt() ;
    while self.tag_opt("(") {
      self.ws_cmt() ;
      let (pos, ident) = self.ident() ? ;
      self.ws_cmt() ;
      let sort = self.sort() ? ;
      self.ws_cmt() ;
      self.tag(")") ? ;
      self.ws_cmt() ;
      let idx = var_map.next_index() ;
      let prev = hash_map.insert(ident, idx) ;
      if prev.is_some() {
        bail!(
          self.error(
            pos, format!(
              "found two quantifier variables named `{}`", conf.bad(ident)
            )
          )
        )
      }
      var_map.push( VarInfo::new(ident.into(), sort, idx) )
    }
    self.tag(")") ? ;
    var_map.shrink_to_fit() ;
    hash_map.shrink_to_fit() ;
    Ok(())
  }

  /// Adds a binding to the current bindings.
  fn insert_bind(
    & mut self, var: & 's str, term: PTTerms
  ) -> Res<()> {
    if let Some(bindings) = self.bindings.last_mut() {
      bindings.insert(var, term) ;
      Ok(())
    } else {
      bail!("bug, adding binding before pushing a binding scope")
    }
  }
  /// Pushes a binding scopes.
  fn push_bind(& mut self) {
    self.bindings.push( HashMap::with_capacity(17) )
  }
  /// Pops a binding scope.
  fn pop_bind(& mut self) -> Res<()> {
    if self.bindings.pop().is_none() {
      bail!("bug, popping binding scope but there's no scope")
    }
    Ok(())
  }
  /// Finds what a variable is mapped to.
  fn get_bind(& self, var: & str) -> Option<& PTTerms> {
    for bindings in self.bindings.iter().rev() {
      if let Some(tterms) = bindings.get(var) {
        return Some(tterms)
      }
    }
    None
  }


  /// Parses the end of some consecutive let-bindings.
  #[inline]
  fn close_let_bindings(& mut self, count: LetCount) -> Res<()> {
    for _ in 0..count.n {
      self.ws_cmt() ;
      self.tag(")") ? ;
      self.pop_bind() ?
    }
    Ok(())
  }




  /// Parses some consecutive let-bindings.
  ///
  /// - open paren,
  /// - `let` keyword, and
  /// - bindings.
  ///
  /// Returns the number of let-bindings it parsed, *i.e.* the number of
  /// corresponding closing parens.
  #[inline]
  fn let_bindings(
    & mut self,
    var_map: & VarInfos,
    map: & HashMap<& 's str, VarIdx>,
    instance: & Instance
  ) -> Res<LetCount> {
    let mut n = 0 ;

    profile! { self tick "parsing", "let bindings" }
    
    'parse_lets: loop {
      conf.check_timeout() ? ;

      if let Some(pos) = self.tag_opt_pos("(") {

        self.ws_cmt() ;
        if self.tag_opt( keywords::let_ ) {
          n += 1 ;
          self.push_bind() ;
          self.ws_cmt() ;
          self.tag("(") ? ;
          self.ws_cmt() ;
          while self.tag_opt("(") {
            self.ws_cmt() ;
            let (_, id) = self.ident() ? ;
            self.ws_cmt() ;
            let tterms = self.parse_ptterms(
              var_map, map, instance
            ) ? ;
            self.insert_bind(id, tterms) ? ;
            self.ws_cmt() ;
            self.tag(")") ? ;
            self.ws_cmt() ;
          }
          self.ws_cmt() ;
          self.tag_err(
            ")", format!(
              "expected binding or `{}` closing the list of bindings",
              conf.emph(")")
            )
          ) ? ;
        } else {
          self.backtrack_to(pos) ;
          break 'parse_lets
        }
      } else {
        break 'parse_lets
      }
      self.ws_cmt()
    }

    profile! { self mark "parsing", "let bindings" }
    profile! { self "let bindings" => add n }

    Ok( LetCount { n } )
  }

  /// Bool parser.
  pub fn bool(& mut self) -> Option<bool> {
    let start_pos = self.pos() ;
    if self.tag_opt("true") {
      if ! self.legal_id_char() {
        Some(true)
      } else {
        self.backtrack_to(start_pos) ;
        None
      }
    } else if self.tag_opt("false") {
      if ! self.legal_id_char() {
        Some(false)
      } else {
        self.backtrack_to(start_pos) ;
        None
      }
    } else {
      None
    }
  }

  /// Numeral parser.
  fn numeral(& mut self) -> Option<Int> {
    let start_pos = self.pos() ;

    if let Some(char) = self.next() {
      if char.is_numeric() {
        // If there's more numbers after this one, then the first one cannot be
        // zero.
        let mut cannot_be_zero = false ;
        while let Some(char) = self.next() {
          if ! char.is_numeric() {
            self.move_back(1) ;
            break
          }
          cannot_be_zero = true ;
        }
        if cannot_be_zero && char == "0" {
          self.backtrack_to(start_pos) ;
          None
        } else {
          Some(
            Int::parse_bytes(
              self.string[
                * start_pos .. self.cursor
              ].as_bytes(), 10
            ).expect("[bug] in integer parsing")
          )
        }
      } else {
        self.backtrack_to(start_pos) ;
        None
      }
    } else {
      None
    }
  }

  /// Decimal parser.
  fn decimal(& mut self) -> Option<Rat> {
    let start_pos = self.pos() ;
    macro_rules! if_not_give_up {
      (( $($cond:tt)* ) => $thing:expr) => (
        if $($cond)* {
          $thing
        } else {
          self.backtrack_to(start_pos) ;
          return None
        }
      )
    }
    let num = if_not_give_up! {
      (let Some(num) = self.numeral()) => num
    } ;
    if_not_give_up! {
      ( self.tag_opt(".") ) => ()
    }
    let mut den: Int = 1.into() ;
    let ten = || consts::ten.clone() ;
    while self.tag_opt("0") { den = den * ten() }
    let dec_start_pos = self.pos() ;
    if let Some(dec) = self.numeral() {
      for _ in * dec_start_pos .. * self.pos() {
        den = den * ten()
      }
      Some( Rat::new( num * den.clone() + dec, den ) )
    } else if den != 1.into() {
      Some( Rat::new(num, 1.into()) )
    } else {
      self.backtrack_to(start_pos) ;
      None
    }
  }

  /// Integer parser (numeral not followed by a `.`).
  pub fn int(& mut self) -> Option<Int> {
    let start_pos = self.pos() ;
    let num = self.numeral() ;
    if num.is_some() && self.peek() == Some(".") {
      self.backtrack_to(start_pos) ;
      return None
    }
    num
  }

  /// Type checks an operator application.
  fn build_app(& self, frame: TermFrame) -> Res<(Term, Pos)> {
    let (op, op_pos, args_pos, args) = frame.destroy() ;

    match term::try_app(op, args) {
      Ok(term) => Ok((term, op_pos)),
      Err(
        term::TypError::Typ { expected, obtained, index }
      ) => if let Some(exp) = expected {
        err_chain! {
          self.error(
            args_pos[index], format!(
              "expected an expression of sort {}, found {}", exp, obtained
            )
          )
          => self.error(op_pos, "in this operator application")
        }
      } else {
        err_chain! {
          self.error(
            args_pos[index], format!(
              "expected the expression starting here has sort {} \
              which is illegal", obtained
            )
          )
          => self.error(op_pos, "in this operator application")
        }
      }
      Err( term::TypError::Msg(blah) ) => bail!(
        self.error(op_pos, blah)
      ),
    }
  }

  /// Real parser.
  ///
  /// Decimal or fraction.
  pub fn real(& mut self) -> Res< Option<Rat> > {
    let start_pos = self.pos() ;

    if let Some(res) = self.decimal() {
      return Ok( Some(res) )
    }

    if self.tag_opt("(") {
      self.ws_cmt() ;
      if self.tag_opt("/") {
        self.ws_cmt() ;
        if let Some(num) = self.numeral() {
          self.tag_opt(".0") ;
          self.ws_cmt() ;
          let den_pos = self.pos() ;
          if let Some(den) = self.numeral() {
            self.tag_opt(".0") ;
            self.ws_cmt() ;
            if self.tag_opt(")") {
              if den.is_zero() {
                bail!(
                  self.error(
                    den_pos, "division by zero is not supported"
                  )
                )
              }
              return Ok(
                Some( Rat::new(num, den) )
              )
            } else {
              bail!(
                self.error(
                  start_pos, "division applied to more than two operands"
                )
              )
            }
          }
        }
      }
    }

    self.backtrack_to(start_pos) ;
    Ok(None)
  }

  // /// Parses an operator or fails.
  // fn op(& mut self) -> Res<Op> {
  //   if let Some(op) = self.op_opt() ? {
  //     Ok(op)
  //   } else {
  //     bail!( self.error_here("expected operator") )
  //   }
  // }

  /// Tries to parse an operator.
  fn op_opt(& mut self) -> Res< Option<Op> > {
    macro_rules! none_if_ident_char_else {
      ($e:expr) => (
        if self.legal_id_char() {
          None
        } else { Some($e) }
      )
    }
    let start_pos = self.pos() ;
    let res = match self.next() {
      Some("a") => if self.tag_opt("nd") {
        none_if_ident_char_else!(Op::And)
      } else {
        None
      },
      Some("o") => if self.tag_opt("r") {
        none_if_ident_char_else!(Op::Or)
      } else {
        None
      },
      Some("n") => if self.tag_opt("ot") {
        none_if_ident_char_else!(Op::Not)
      } else {
        None
      },
      Some("i") => if self.tag_opt("te") {
        none_if_ident_char_else!(Op::Ite)
      } else {
        None
      },
      Some("m") => if self.tag_opt("od") {
        none_if_ident_char_else!(Op::Mod)
      } else {
        None
      },
      Some("r") => if self.tag_opt("em") {
        none_if_ident_char_else!(Op::Rem)
      } else {
        None
      },
      Some("d") => if self.tag_opt("iv") {
        none_if_ident_char_else!(Op::IDiv)
      } else if self.tag_opt("istinct") {
        none_if_ident_char_else!(Op::Distinct)
      } else {
        None
      },
      Some("t") => if self.tag_opt("o_int") {
        none_if_ident_char_else!(Op::ToInt)
      } else if self.tag_opt("o_real") {
        none_if_ident_char_else!(Op::ToReal)
      } else {
        None
      },

      Some("s") => if self.tag_opt("tore") {
        none_if_ident_char_else!(Op::Store)
      } else if self.tag_opt("elect") {
        none_if_ident_char_else!(Op::Select)
      } else {
        None
      },

      Some("=") => if self.tag_opt(">") {
        Some(Op::Impl)
      } else {
        Some(Op::Eql)
      },
      Some(">") => if self.tag_opt("=") {
        Some(Op::Ge)
      } else {
        Some(Op::Gt)
      },
      Some("<") => if self.tag_opt("=") {
        Some(Op::Le)
      } else {
        Some(Op::Lt)
      },
      Some("+") => Some(Op::Add),
      Some("-") => Some(Op::Sub),
      Some("*") => Some(Op::Mul),
      Some("/") => Some(Op::Div),
      Some(_) => None,
      None => None,
    } ;

    if res.is_none() {
      self.backtrack_to(start_pos)
    }

    Ok( res )
  }

  /// Parses a single term.
  ///
  /// # TODO
  ///
  /// - remove the recursive call for arrays
  pub fn term_opt(
    & mut self,
    var_map: & VarInfos,
    map: & HashMap<& 's str, VarIdx>,
    instance: & Instance
  ) -> Res< Option<Term> > {
    debug_assert! { self.cxt.term_stack.is_empty() }
    conf.check_timeout() ? ;
    let start_pos = self.pos() ;

    // The correct (non-error) way to exit this loop is
    //
    // `break 'read_kids <val>`
    //
    // If `<val> == None`, the code below will automatically backtrack to
    // `start_pos` and clear the `term_stack`, so there's no need to do it in
    // the loop.
    let res = 'read_kids: loop {

      let bind_count = self.let_bindings(var_map, map, instance) ? ;

      self.ws_cmt() ;
      let mut term_pos = self.pos() ;
      let mut term = if let Some(int) = self.int() {
        term::int(int)
      } else if let Some(real) = self.real() ? {
        term::real(real)
      } else if let Some(b) = self.bool() {
        term::bool(b)
      } else if let Some((pos, id)) = self.ident_opt() ? {

        if let Some(idx) = map.get(id) {
          term::var(* idx, var_map[* idx].typ.clone())

        } else if let Some(ptterms) = self.get_bind(id) {
          if let Some(term) = ptterms.to_term().chain_err(
            || format!("while retrieving binding for {}", conf.emph(id))
          ) ? {
            term
          } else {
            // Not in a legal term.
            break 'read_kids None
          }

        } else if self.cxt.pred_name_map.get(id).is_some() {
          // Identifier is a predicate, we're not in a legal term.
          break 'read_kids None

        } else if let Some(datatype) = dtyp::of_constructor(id) {
          if let Some(constructor) = datatype.news.get(id) {
            if constructor.is_empty() {
              bail!(
                self.error(pos, "term for datatypes isn't implemented")
              )
            } else {
              bail!(
                self.error(
                  pos, format!(
                    "constructor `{}` of datatype `{}` takes {} value(s), \
                    applied here to none",
                    conf.bad(id), conf.emph(& datatype.name), constructor.len()
                  )
                )
              )
            }
          } else {
            bail!("inconsistent datatype map internal state")
          }

        } else {
          bail!(
            self.error(
              pos, format!("unknown identifier `{}`", conf.bad(id))
            )
          )
        }

      } else if self.tag_opt("(") {

        self.ws_cmt() ;
        let op_pos = self.pos() ;

        if let Some(op) = self.op_opt() ? {
          let frame = TermFrame::new(op, op_pos, bind_count) ;
          self.cxt.term_stack.push(frame) ;
          continue 'read_kids

        } else if self.tag_opt("(") {

          // Try to parse a constant array.
          if self.tag_opt("as")
          && { self.ws_cmt() ; self.tag_opt("const") } {
            self.ws_cmt() ;
            let sort_pos = self.pos() ;
            let typ = self.sort() ? ;
            let (src, tgt) = if let Some((src, tgt)) = typ.array_inspect() {
              (src, tgt)
            } else {
              bail!(
                self.error(sort_pos, "expected array sort")
              )
            } ;

            self.ws_cmt() ;
            self.tag(")") ? ;
            self.ws_cmt() ;

            let term_pos = self.pos() ;

            let stack = Vec::with_capacity(
              self.cxt.term_stack.capacity()
            ) ;
            let old_stack = ::std::mem::replace(
              & mut self.cxt.term_stack, stack
            ) ;

            // !!!! RECURSIVE CALL !!!!
            if let Some(term) = self.term_opt(var_map, map, instance) ? {
              if term.typ() != * tgt {
                bail!(
                  self.error(
                    term_pos, format!(
                      "expected expression of sort {}, got one of sort {}",
                      tgt, term.typ()
                    )
                  )
                )
              }

              let empty_stack = ::std::mem::replace(
                & mut self.cxt.term_stack,
                old_stack
              ) ;
              debug_assert! { empty_stack.is_empty() }

              self.ws_cmt() ;
              self.tag(")") ? ;
              term::cst_array(src.clone(), term)

            } else {
              bail!(
                self.error_here("expected term")
              )
            }

          } else {
            bail!( self.error_here("unexpected token") )
          }

        } else if let Some((pos, id)) = self.ident_opt().chain_err(
          || "while trying to parse datatype"
        ) ? {
          let mut trm: Option<Term> = None ;
          if let Some(datatype) = dtyp::of_constructor(id) {
            if let Some(_constructor) = datatype.news.get(id) {
              bail!(
                self.error(pos, "term for datatypes isn't implemented")
              )
            }
          }
          if let Some(trm) = trm {
            trm
          } else if self.cxt.term_stack.is_empty() {
            self.backtrack_to(pos) ;
            break 'read_kids None
          } else {
            bail!(
              self.error(
                pos, format!( "unknown identifier `{}`", conf.bad(id) )
              )
            )
          }

        } else if self.cxt.term_stack.is_empty() {
          break 'read_kids None

        } else {
          bail!( self.error_here("unexpected token") )
        }

      } else {
        break 'read_kids None

      } ;

      'go_up: while let Some(mut frame) = self.cxt.term_stack.pop() {
        self.ws_cmt() ;

        frame.push_arg(term_pos, term) ;

        if self.tag_opt(")") {
          if frame.is_empty() {
            bail!(
              self.error(
                frame.op_pos, format!(
                  "Illegal nullary application of operator `{}`",
                  conf.bad( frame.op.as_str() )
                )
              )
            )
          }

          let bind_count = frame.let_count() ;
          let (nu_term, nu_term_pos) = self.build_app(frame) ? ;
          term = nu_term ;
          term_pos = nu_term_pos ;
          self.ws_cmt() ;

          self.close_let_bindings(bind_count) ? ;
          continue 'go_up

        } else {
          self.cxt.term_stack.push(frame) ;
          continue 'read_kids
        }
      }

      // Stack is empty, done.
      debug_assert!( self.cxt.term_stack.is_empty() ) ;
      break 'read_kids Some(term)
    } ;

    if res.is_none() {
      self.cxt.term_stack.clear() ;
      self.backtrack_to(start_pos) ;
    }

    Ok(res)
  }


  /// Tries to parse a `define-fun`.
  fn define_fun(
    & mut self, instance: & mut Instance
  ) -> Res<bool> {
    if ! self.tag_opt(keywords::cmd::def_fun) {
      return Ok(false)
    }
    conf.check_timeout() ? ;
    self.ws_cmt() ;

    let (name_pos, name) = self.ident() ? ;
    self.ws_cmt() ;

    let mut var_info = VarInfos::new() ;
    let mut map = HashMap::new() ;
    self.args(& mut var_info, & mut map) ? ;
    self.ws_cmt() ;

    let sort_pos = self.pos() ;
    let out_sort = self.sort() ? ;
    self.ws_cmt() ;

    let body_pos = self.pos() ;
    let body = self.parse_ptterms(& var_info, & map, instance) ? ;
    self.ws_cmt() ;

    if out_sort != body.typ() {
      Err::<_, Error>(
        self.error(
          name_pos, format!("in this `define-fun` for {}", conf.emph(name))
        ).into()
      ).chain_err(
        || self.error(body_pos, "body is ill typed")
      ).chain_err(
        || self.error(
          sort_pos, format!(
            "it has type {}, but expected {} as specified",
            conf.emph(& format!("{}", body.typ())),
            conf.emph(& format!("{}", out_sort))
          )
        )
      ) ?
    }

    let prev = instance.add_define_fun(name, var_info, body) ;

    if prev.is_some() {
      bail!(
        self.error(name_pos, format!("redefinition of {}", conf.emph(name)))
      )
    }

    Ok(true)
  }


  /// Parses some PTTerm arguments.
  fn ptterm_args(
    & mut self,
    var_map: & VarInfos,
    map : & HashMap<& 's str, VarIdx>,
    instance: & Instance
  ) -> Res< VarMap<(Pos, PTTerms)> > {
    let mut res = VarMap::with_capacity(11) ;

    let mut backtrack_pos = self.pos() ;
    let mut term_pos = self.pos() ;

    while ! self.tag_opt(")") {
      conf.check_timeout() ? ;
      let ptterms = self.parse_ptterms(
        var_map, map, instance
      ) ? ;
      res.push((term_pos, ptterms)) ;
      backtrack_pos = self.pos() ;
      self.ws_cmt() ;
      term_pos = self.pos()
    }

    self.backtrack_to(backtrack_pos) ;

    res.shrink_to_fit() ;

    Ok(res)
  }


  /// Parses arguments for a predicate application and type-checks it.
  fn pred_args(
    & mut self,
    pred: PrdIdx,
    pred_pos: Pos,
    var_map: & VarInfos,
    map: & HashMap<& 's str, VarIdx>,
    instance: & Instance
  ) -> Res< Option<PTTerms> > {
    let mut args = VarMap::with_capacity(11) ;
    let mut kid_pos = Vec::with_capacity(11) ;

    let mut backtrack_pos = self.pos() ;
    let mut term_pos = self.pos() ;

    while let Some(term) = self.term_opt(
      var_map, map, instance
    ) ? {
      kid_pos.push(term_pos) ;
      args.push(term) ;
      backtrack_pos = self.pos() ;
      self.ws_cmt() ;
      term_pos = self.pos()
    }

    self.backtrack_to(backtrack_pos) ;

    args.shrink_to_fit() ;

    let sig = & instance[pred].sig ;

    if sig.len() != kid_pos.len() {
      bail!(
        self.error(
          pred_pos, format!(
            "predicate {} takes {} arguments, but is applied to {}",
            conf.emph(& instance[pred].name), sig.len(), kid_pos.len()
          )
        )
      )
    } else {
      for ((index, exp), pos) in sig.index_iter().zip( kid_pos.into_iter() ) {
        let found = args[index].typ() ;
        if exp != & found {
          err_chain! {
            self.error(
              pos, format!(
                "expected an expression of sort {}, found {} ({})",
                exp, & args[index], found
              )
            )
            => self.error(
              pred_pos, format!(
                "in this application of {}, parameter #{}",
                conf.emph(& instance[pred].name), index
              )
            )
          }
        }
      }
    }

    Ok(
      Some(
        PTTerms::tterm( TTerm::P { pred, args: args.into() } )
      )
    )
  }


  /// Parses a top term or fails.
  #[allow(dead_code)]
  fn top_term(
    & mut self,
    var_map: & VarInfos,
    map: & HashMap<& 's str, VarIdx>,
    instance: & Instance,
  ) -> Res<PTTerms> {
    if let Some(res) = self.top_term_opt(var_map, map, instance) ? {
      Ok(res)
    } else {
      bail!( self.error_here("expected term") )
    }
  }
  /// Tries to parse a top term.
  fn top_term_opt(
    & mut self,
    var_map: & VarInfos,
    map: & HashMap<& 's str, VarIdx>,
    instance: & Instance,
  ) -> Res< Option< PTTerms > > {
    conf.check_timeout() ? ;
    let bind_count = self.let_bindings(var_map, map, instance) ? ;

    self.ws_cmt() ;
    let start_pos = self.pos() ;

    let res = if let Some(term) = self.term_opt(
      var_map, map, instance
    ) ? {
      Ok( Some(
        PTTerms::tterm( TTerm::T( term ) )
      ) )
    } else if let Some((pos, id)) = self.ident_opt().chain_err(
      || "while trying to parse a top term (1)"
    ) ? {
      if let Some(idx) = self.cxt.pred_name_map.get(id) {
        let idx = * idx ;
        if instance[idx].sig.is_empty() {
          Ok( Some(
            PTTerms::TTerm(
              TTerm::P { pred: idx, args: VarMap::with_capacity(0).into() }
            )
          ) )
        } else {
          bail!(
            self.error(
              pos, format!(
                "illegal nullary application of predicate `{}`, \
                this predicate takes {} arguments",
                conf.bad(& instance[idx].name), instance[idx].sig.len()
              )
            )
          )
        }

      } else if let Some(ptterms) = self.get_bind(id) {
        Ok( Some( ptterms.clone() ) )

      } else {
        bail!(
          self.error(
            pos, format!(
              "unknown ident `{}`", conf.bad(id)
            )
          )
        )
      }
    } else if self.tag_opt("(") {
      self.ws_cmt() ;

      if self.tag_opt(keywords::forall)
      || self.tag_opt(keywords::exists) {
        bail!(
          self.error(
            start_pos,
            "unable to work on clauses that are not ground".to_string()
          )
        )
      } else if let Some((ident_pos, ident)) = self.ident_opt().chain_err(
        || "while trying to parse a top term (2)"
      ) ? {

        if let Some(idx) = self.cxt.pred_name_map.get(ident).cloned() {
          let res = self.pred_args(idx, ident_pos, var_map, map, instance) ? ;
          self.ws_cmt() ;
          self.tag(")") ? ;
          Ok(res)
        } else if let Some(
          & (ref var_info, ref body)
        ) = instance.get_define_fun(ident) {
          // Parse arguments.
          self.ws_cmt() ;
          let args = self.ptterm_args(var_map, map, instance) ? ;
          self.ws_cmt() ;
          self.tag(")") ? ;

          if var_info.len() != args.len() {
            bail!(
              self.error(
                ident_pos, format!(
                  "wrong number of arguments, expected {} but got {}",
                  var_info.len(), args.len()
                )
              )
            )
          }

          for (var, info) in var_info.index_iter() {
            if info.typ != args[var].1.typ() {
              bail!(
                self.error(
                  args[var].0, format!(
                    "sort error, expected term of sort {}, found {}",
                    info.typ, args[var].1.typ()
                  )
                )
              )
            }
          }

          let args: VarMap<_> = args.into_iter().map(|(_, t)| t).collect() ;

          let res = body.subst_total(& args).chain_err(
            || self.error(
              ident_pos, format!(
                "while inlining the application of {}", conf.emph(ident)
              )
            )
          ) ? ;

          Ok( Some(res) )
        } else {
          bail!(
            self.error(
              ident_pos,
              format!("unknown identifier `{}`", conf.bad(ident))
            )
          )
        }
      } else {
        bail!(
          self.error_here("expected operator, let binding or predicate")
        )
      }

    } else {
      // In theory, we should check if the top term is an ident that's either a
      // quantified or bound variable. In practice, this is done at the level
      // above this one, in `parse_ptterms`.
      Ok(None)
    } ;

    self.ws_cmt() ;
    self.close_let_bindings(bind_count) ? ;

    res
  }


  /// Parses some top terms (parsing variant, for simplifications).
  fn parse_ptterms(
    & mut self,
    var_map: & VarInfos,
    map: & HashMap<& 's str, VarIdx>,
    instance: & Instance,
  ) -> Res<PTTerms> {
    enum Frame {
      And(Vec<PTTerms>),
      Or(Vec<PTTerms>),
      Impl(Vec<PTTerms>),
      Not,
      Let(LetCount)
    }
    let mut stack: Vec<Frame> = vec![] ;

    'go_down: loop {

      self.ws_cmt() ;

      let bind_count = self.let_bindings(var_map, & map, instance) ? ;
      if ! bind_count.is_zero() {
        stack.push( Frame::Let(bind_count) ) ;
      }

      self.ws_cmt() ;

      let start_pos = self.pos() ;
      let mut ptterm = if let Some(pos) = self.tag_opt_pos("(") {

        self.ws_cmt() ;

        if self.tag_opt("and") {
          stack.push( Frame::And(vec![]) ) ;
          continue 'go_down
        } else if self.tag_opt("or") {
          stack.push( Frame::Or(vec![]) ) ;
          continue 'go_down
        } else if self.tag_opt("not") {
          stack.push( Frame::Not ) ;
          continue 'go_down
        } else if self.tag_opt("=>") {
          stack.push( Frame::Impl(vec![]) ) ;
          continue 'go_down
        } else {
          self.backtrack_to(pos) ;
          if let Some(top) = self.top_term_opt(
            var_map, & map, instance
          ) ? {
            if top.typ().is_bool() {
              top
            } else if stack.is_empty() {
              // If we get here, it means what we're parsing does not have type
              // bool. Which means we're not inside a top-term (we're most
              // likely parsing a let-binding).
              return Ok(top)
            } else {
              err_chain! {
                "while parsing top term"
                => self.error(
                  start_pos, format!(
                    "expected expression of type Bool, found {}", top.typ()
                  )
                )
              }
            }
          } else if let Some(top) = self.term_opt(
            var_map, & map, instance
          ) ? {
            if top.typ().is_bool() {
              PTTerms::TTerm( TTerm::T(top) )
            } else if stack.is_empty() {
              // If we get here, it means what we're parsing does not have type
              // bool. Which means we're not inside a top-term (we're most
              // likely parsing a let-binding).
              return Ok(
                PTTerms::TTerm( TTerm::T(top) )
              )
            } else {
              err_chain! {
                "while parsing subterm"
                => self.error(
                  start_pos, format!(
                    "expected expression of type Bool, found {}", top.typ()
                  )
                )
              }
            }
          } else {
            bail!(
              self.error(
                start_pos, "failed to parse expression top term"
              )
            )
          }
        }
      } else if let Some(top) = self.top_term_opt(
        var_map, & map, instance
      ) ? {
        if top.typ().is_bool() {
          top
        } else if stack.is_empty() {
          // If we get here, it means what we're parsing does not have type
          // bool. Which means we're not inside a top-term (we're most likely
          // parsing a let-binding).
          return Ok(top)
        } else {
          err_chain! {
            "while parsing top term"
            => self.error(
              start_pos, format!(
                "expected expression of type Bool, found {}", top.typ()
              )
            )
          }
        }
      } else if let Some(top) = self.term_opt(
        var_map, & map, instance
      ) ? {
        if top.typ().is_bool() {
          PTTerms::TTerm( TTerm::T(top) )
        } else if stack.is_empty() {
          // If we get here, it means what we're parsing does not have type
          // bool. Which means we're not inside a top-term (we're most likely
          // parsing a let-binding).
          return Ok(
            PTTerms::TTerm( TTerm::T(top) )
          )
        } else {
          err_chain! {
            "while parsing subterm (ident or constant)"
            => self.error(
              start_pos, format!(
                "expected expression of type Bool, found {}", top.typ()
              )
            )
          }
        }
      } else {
        bail!(
          self.error(
            start_pos, "failed to parse top expression"
          )
        )
      } ;

      'go_up: loop {
        match stack.pop() {
          Some( Frame::And(mut args) ) => {
            args.push(ptterm) ;
            self.ws_cmt() ;
            if self.tag_opt(")") {
              ptterm = PTTerms::and(args) ;
              continue 'go_up
            } else {
              stack.push( Frame::And(args) ) ;
              continue 'go_down
            }
          },
          Some( Frame::Or(mut args) ) => {
            args.push(ptterm) ;
            self.ws_cmt() ;
            if self.tag_opt(")") {
              ptterm = PTTerms::or(args) ;
              continue 'go_up
            } else {
              stack.push( Frame::Or(args) ) ;
              continue 'go_down
            }
          },
          Some( Frame::Impl(mut args) ) => {
            args.push(ptterm) ;
            self.ws_cmt() ;
            if self.tag_opt(")") {
              if args.len() != 2 {
                bail!(
                  "unexpected implication over {} (!= 2) arguments", args.len()
                )
              }
              let (rhs, lhs) = (args.pop().unwrap(), args.pop().unwrap()) ;
              ptterm = PTTerms::or( vec![ PTTerms::not(lhs) ?, rhs ] ) ;
              continue 'go_up
            } else {
              stack.push( Frame::Impl(args) ) ;
              continue 'go_down
            }
          },
          Some( Frame::Not ) => {
            self.ws_cmt() ;
            ptterm = PTTerms::not(ptterm) ? ;
            self.tag(")") ? ;
            continue 'go_up
          },
          Some( Frame::Let(bind_count) ) => {
            self.close_let_bindings(bind_count) ? ;
            continue 'go_up
          },
          None => break 'go_down Ok(ptterm),
        }
      }

    }
  }


  /// Parses a forall.
  ///
  /// Returns
  ///
  /// - `None` if nothing was parsed ;
  /// - `Some(None)` if a clause was parsed but it was not actually added
  ///   (*e.g.* redundant) ;
  /// - `Some(idx)` if a clause was parsed and added, and it has index `idx`.
  fn forall(
    & mut self, instance: & mut Instance
  ) -> Res< Option<ClauseRes> > {
    if ! self.tag_opt(keywords::forall) {
      return Ok(None)
    }

    let (mut var_map, mut hash_map, mut parse_args, mut closing_parens) = (
      VarMap::with_capacity(11), HashMap::with_capacity(11), true, 0
    ) ;

    while parse_args {
      self.ws_cmt() ;
      self.args(& mut var_map, & mut hash_map) ? ;

      self.ws_cmt() ;
      parse_args = if let Some(pos) = self.tag_opt_pos("(") {
        self.ws_cmt() ;
        if self.tag_opt(keywords::forall) {
          closing_parens += 1 ;
          true
        } else {
          self.backtrack_to(pos) ;
          false
        }
      } else {
        false
      }
    }

    self.ws_cmt() ;
    let outter_bind_count = self.let_bindings(
      & var_map, & hash_map, instance
    ) ? ;

    self.ws_cmt() ;
    let idx = self.parse_clause(
      var_map, & hash_map, instance, false
    ) ? ;

    self.ws_cmt() ;
    self.close_let_bindings(outter_bind_count) ? ;

    for _ in 0..closing_parens {
      self.ws_cmt() ;
      self.tag(")") ?
    }

    Ok( Some(idx) )
  }


  /// Parses a negated exists.
  ///
  /// Returns
  ///
  /// - `None` if nothing was parsed ;
  /// - `Some(None)` if a clause was parsed but it was not actually added
  ///   (*e.g.* redundant) ;
  /// - `Some(idx)` if a clause was parsed and added, and it has index `idx`.
  fn nexists(
    & mut self, instance: & mut Instance
  ) -> Res< Option<ClauseRes> > {
    if ! self.tag_opt(keywords::op::not_) {
      return Ok(None)
    }
    self.ws_cmt() ;
    let outter_bind_count = self.let_bindings(
      & VarMap::new(), & HashMap::new(), instance
    ) ? ;

    self.ws_cmt() ;
    self.tag("(") ? ;

    self.ws_cmt() ;
    self.tag(keywords::exists) ? ;

    let (mut var_map, mut hash_map, mut parse_args, mut closing_parens) = (
      VarMap::with_capacity(11), HashMap::with_capacity(11), true, 0
    ) ;

    while parse_args {
      self.ws_cmt() ;
      self.args(& mut var_map, & mut hash_map) ? ;

      self.ws_cmt() ;
      parse_args = if let Some(pos) = self.tag_opt_pos("(") {
        self.ws_cmt() ;
        if self.tag_opt(keywords::exists) {
          closing_parens += 1 ;
          true
        } else {
          self.backtrack_to(pos) ;
          false
        }
      } else {
        false
      }
    }

    self.ws_cmt() ;
    let idx = self.parse_clause(var_map, & hash_map, instance, true) ? ;

    self.ws_cmt() ;
    self.tag(")") ? ;

    self.ws_cmt() ;
    self.close_let_bindings(outter_bind_count) ? ;

    for _ in 0..closing_parens {
      self.ws_cmt() ;
      self.tag(")") ?
    }

    Ok( Some(idx) )
  }


  fn parse_clause(
    & mut self,
    var_map: VarInfos,
    map: & HashMap<& 's str, VarIdx>,
    instance: & mut Instance,
    negated: bool,
  ) -> Res< ClauseRes > {
    profile! { self tick "parsing", "clause" }
    self.ws_cmt() ;

    let start_pos = self.pos() ;
    let mut ptterms = self.parse_ptterms(
      & var_map, & map, instance
    ) ? ;
    if ! ptterms.typ().is_bool() {
      err_chain! {
        "while parsing clause terms"
        => self.error(
          start_pos, format!(
            "expected expression of type Bool, got {}", ptterms.typ()
          )
        )
      }
    }
    if negated {
      ptterms = PTTerms::not(ptterms) ?
    }

    let (mut at_least_one, idx) = (
      false, instance.next_clause_index()
    ) ;

    let mut clauses = ptterms.into_clauses()?.into_iter() ;

    if let Some((last_lhs, last_rhs)) = clauses.next() {

      for (lhs, rhs) in clauses {
        if self.add_clause(instance, var_map.clone(), lhs, rhs) ? {
          at_least_one = true
        }
      }
      if self.add_clause(instance, var_map, last_lhs, last_rhs) ? {
        at_least_one = true
      }
    }

    profile! { self mark "parsing", "clause" }

    if at_least_one {
      Ok( ClauseRes::Added(idx) )
    } else {
      Ok( ClauseRes::Skipped )
    }
  }

  /// Adds a clause to an instance.
  fn add_clause(
    & self, instance: & mut Instance,
    var_map: VarInfos, lhs: Vec<TTerm>, rhs: TTerm
  ) -> Res<bool> {

    let mut nu_lhs = Vec::with_capacity( lhs.len() ) ;
    let mut lhs_is_false = false ;
    for lhs in lhs {
      if ! lhs.is_true() {
        if lhs.is_false() {
          lhs_is_false = true ;
          break
        } else {
          nu_lhs.push(lhs)
        }
      }
    }
    let rhs = match rhs {
      TTerm::P { pred, args } => Some((pred, args)),
      TTerm::T(t) => {
        if t.bool() != Some(false) {
          nu_lhs.push( TTerm::T( term::not(t) ) )
        }
        None
      },
    } ;

    if ! lhs_is_false {
      profile! { self tick "parsing", "add clause" }
      let maybe_index = instance.push_new_clause(
        var_map, nu_lhs, rhs, "parsing"
      ) ? ;
      profile! { self mark "parsing", "add clause" }
      Ok(maybe_index.is_some())
    } else {
      Ok(false)
    }
  }


  /// Parses an assert.
  fn assert(& mut self, instance: & mut Instance) -> Res<bool> {
    if ! self.tag_opt(keywords::cmd::assert) {
      return Ok(false)
    }

    profile! { self tick "parsing", "assert" }

    self.ws_cmt() ;

    let start_pos = self.pos() ;
    let tagged = if self.tag_opt("(") {
      self.ws_cmt() ;
      if self.tag_opt("!") {
        self.ws_cmt() ;
        true
      } else {
        self.backtrack_to(start_pos) ;
        false
      }
    } else {
      false
    } ;

    let bind_count = self.let_bindings(
      & VarMap::new(), & HashMap::new(), instance
    ) ? ;

    let idx = if self.tag_opt("(") {
      self.ws_cmt() ;

      let idx = if let Some(idx) = self.forall(instance) ? {
        idx
      } else if let Some(idx) = self.nexists(instance) ? {
        idx
      } else {
        bail!(
          self.error_here("expected forall or negated exists")
        )
      } ;

      self.ws_cmt() ;
      self.tag(")") ? ;
      idx
    } else if self.tag_opt("true") {
      ClauseRes::Skipped
    } else if self.tag_opt("false") {
      instance.set_unsat() ;
      ClauseRes::Skipped
    } else {
      bail!(
        self.error_here("expected negation, qualifier, `true` or `false`")
      )
    } ;

    self.ws_cmt() ;
    self.close_let_bindings(bind_count) ? ;

    if tagged {
      self.ws_cmt() ;
      self.tag(":named").chain_err(
        || "unexpected tag"
      ) ? ;
      self.ws_cmt() ;
      let (_, ident) = self.ident().chain_err(
        || "expected identifier after `:named` tag"
      ) ? ;
      if let Some(idx) = idx.into_option() {
        instance.set_old_clause_name(idx, ident.into()) ?
      }
      self.ws_cmt() ;
      self.tag(")") ? ;
    }

    profile! { self mark "parsing", "assert" }

    Ok(true)
  }

  /// Parses a check-sat.
  fn check_sat(& mut self) -> bool {
    self.tag_opt(keywords::cmd::check_sat)
  }

  /// Parses a get-model.
  fn get_model(& mut self) -> bool {
    self.tag_opt(keywords::cmd::get_model)
  }

  /// Parses a get-unsat-core.
  fn get_unsat_core(& mut self) -> bool {
    self.tag_opt(keywords::cmd::get_unsat_core)
  }

  /// Parses a get-proof.
  fn get_proof(& mut self) -> bool {
    self.tag_opt(keywords::cmd::get_proof)
  }

  /// Parses an exit command.
  fn exit(& mut self) -> bool {
    self.tag_opt(keywords::cmd::exit)
  }

  /// Parses an reset command.
  fn reset(& mut self) -> bool {
    self.tag_opt(keywords::cmd::reset)
  }

  /// Parses items, returns true if it found a check-sat.
  pub fn parse(
    mut self, instance: & mut Instance
  ) -> Res<Parsed> {
    self.ws_cmt() ;
    let mut res = Parsed::Eof ;
    self.cxt.term_stack.clear() ;

    while self.has_next() {
      self.ws_cmt() ;
      self.tag_err(
        "(", format!(
          "expected `{}` opening top-level item",
          conf.emph("(")
        )
      ) ? ;
      self.ws_cmt() ;

      let start_pos = self.pos() ;

      res = if self.set_info() ? {
        Parsed::Items
      } else if let Some((key, val)) = self.set_option() ? {
        instance.set_option(key, val).chain_err(
          || {
            self.backtrack_to(start_pos) ;
            self.error_here("in this set-option")
          }
        ) ? ;
        Parsed::Items
      } else if self.set_logic() ?
      || self.pred_dec(instance) ?
      || self.define_fun(instance) ?
      || self.assert(instance) ?
      || self.dtyp_dec() ? {
        Parsed::Items
      } else if self.check_sat() {
        Parsed::CheckSat
      } else if self.get_model() {
        Parsed::GetModel
      } else if self.get_unsat_core() {
        Parsed::GetUnsatCore
      } else if self.get_proof() {
        Parsed::GetProof
      } else if self.exit() {
        Parsed::Exit
      } else if self.reset() {
        Parsed::Reset
      } else if let Some(blah) = self.echo() ? {
        println!("{}", blah) ;
        Parsed::Items
      } else {
        bail!(
          self.error_here("expected top-level item")
        )
      } ;

      self.ws_cmt() ;
      self.tag(")") ? ;
      self.ws_cmt() ;

      debug_assert!( self.cxt.term_stack.is_empty() ) ;
      debug_assert!( self.cxt.mem.is_empty() ) ;

      if res != Parsed::Items {
        return Ok(res)
      }
    }

    debug_assert!( self.cxt.term_stack.is_empty() ) ;
    debug_assert!( self.cxt.mem.is_empty() ) ;

    Ok(res)
  }
}



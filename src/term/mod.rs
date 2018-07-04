//! Hashconsed terms.
//!
//! # Terms
//!
//! The factory is a `static_ref` for easy creation. The `R`eal term structure
//! is [`RTerm`](enum.RTerm.html) which is hashconsed into
//! [`Term`](type.Term.html). The factory
//! ([`HashConsign`](https://crates.io/crates/hashconsing)) is not directly
//! accessible. Terms are created *via* the functions in this module, such as
//! [var](fn.var.html), [int](fn.int.html), [app](fn.app.html), *etc.*
//!
//! Terms are not typed at all. A predicate application is **not** a term, only
//! operator applications are.
//!
//! Terms are simplified (when possible) at creation. In particular, the order
//! of the arguments can change, double negations will be simplified, *etc.*
//! See [`normalize`](fn.normalize.html) for more details.
//!
//! # Top-level terms
//!
//! A [`TTerm`](enum.tterm.html) is either a term or a predicate application to
//! some terms. Top-level terms are not hashconsed as they are shallow.
//!
//! # Variables
//!
//! A variable is a `usize` wrapped in a zero-cost
//! [`VarIdx`](../common/struct.VarIdx.html) for safety. It has no semantics at
//! all by itself. Variables are given meaning by
//!
//! - the `sig` field of a [`PrdInfo`](../instance/info/struct.PrdInfo.html),
//!   which gives them types;
//! - the [`VarInfo`s](../instance/info/struct.VarInfo.html) stored in a
//!   [`Clause`](../instance/struct.Clause.html), which give them a name and a
//!   type.
//!
//! # Examples
//!
//! ```rust
//! # use hoice::term ;
//! # use hoice::term::{ Op, RTerm, typ } ;
//! let some_term = term::eq(
//!   term::int(11), term::app(
//!     Op::Mul, vec![ term::int_var(5), term::int(2) ]
//!   )
//! ) ;
//! # println!("{}", some_term) ;
//! 
//! // A `Term` dereferences to an `RTerm`:
//! match * some_term {
//!   RTerm::App { ref typ, op: Op::Eql, ref args } => {
//!     assert_eq!( typ, & typ::bool() ) ;
//!     assert_eq!( args.len(), 2 ) ;
//!     assert_eq!( format!("{}", some_term), "(= (+ (* (- 2) v_5) 11) 0)" )
//!   },
//!   _ => panic!("not an equality"),
//! }
//! ```

use hashconsing::* ;

use common::* ;

#[macro_use]
mod op ;
mod factory ;
mod tterms ;
pub mod simplify ;
pub mod typ ;
mod fold ;
mod leaf_iter ;

pub use self::op::* ;
pub use self::factory::* ;
pub use self::tterms::* ;
pub use self::typ::Typ ;
pub use self::leaf_iter::LeafIter ;

#[cfg(test)]
mod test ;



/// Hash consed term.
pub type Term = HConsed<RTerm> ;



/// A real term.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum RTerm {
  /// A clause variable.
  Var(Typ, VarIdx),
  /// A constant.
  Cst(Val),

  /// A **constant** array.
  ///
  /// The type is the type of **the indices** of the arrays.
  CArray {
    /// Type of **the indices** (not the array).
    typ: Typ,
    /// Default term of the array.
    term: Term
  },

  /// An operator application.
  App {
    /// Type of the application.
    typ: Typ,
    /// The operator.
    op: Op,
    /// The arguments.
    args: Vec<Term>,
  },

  /// A datatype constructor application.
  DTypNew {
    /// Type of the application.
    typ: Typ,
    /// Name of the constructor.
    name: String,
    /// Arguments of the constructor.
    args: Vec<Term>,
  },

  /// A datatype selector application.
  DTypSlc {
    /// Type of the application.
    typ: Typ,
    /// Name of the selector.
    name: String,
    /// Argument of the selector.
    term: Term,
  },
}



impl RTerm {
  /// The operator and the kids of a term.
  pub fn app_inspect(& self) -> Option< (Op, & Vec<Term>) > {
    match * self {
      RTerm::App { op, ref args, .. } => Some((op, args)),
      _ => None,
    }
  }

  /// Returns the kids of an ite.
  pub fn ite_inspect(& self) -> Option<(& Term, & Term, & Term)> {
    match * self {
      RTerm::App { op: Op::Ite, ref args, .. } => {
        debug_assert_eq! { args.len(), 3 }
        Some( (& args[0], & args[1], & args[2]) )
      },
      _ => None,
    }
  }

  /// Returns the kid of a negation.
  pub fn neg_inspect(& self) -> Option<& Term> {
    match * self {
      RTerm::App { op: Op::Not, ref args, .. } => {
        debug_assert_eq! { args.len(), 1 }
        Some(& args[0])
      },
      _ => None,
    }
  }

  /// Returns the kids of conjunctions.
  pub fn conj_inspect(& self) -> Option<& Vec<Term>> {
    match * self {
      RTerm::App { op: Op::And, ref args, .. } => Some(args),
      _ => None,
    }
  }
  /// Returns the kids of disjunctions.
  pub fn disj_inspect(& self) -> Option<& Vec<Term>> {
    match * self {
      RTerm::App { op: Op::Or, ref args, .. } => Some(args),
      _ => None,
    }
  }
  /// Returns the kids of equalities.
  pub fn eq_inspect(& self) -> Option<& Vec<Term>> {
    match * self {
      RTerm::App { op: Op::Eql, ref args, .. } => Some(args),
      _ => None,
    }
  }
  /// Returns the kids of additions.
  pub fn add_inspect(& self) -> Option<& Vec<Term>> {
    match * self {
      RTerm::App { op: Op::Add, ref args, .. } => Some(args),
      _ => None,
    }
  }
  /// Returns the kids of subtractions.
  pub fn sub_inspect(& self) -> Option<& Vec<Term>> {
    match * self {
      RTerm::App { op: Op::Sub, ref args, .. } => Some(args),
      _ => None,
    }
  }
  /// Returns the kids of multiplications.
  pub fn mul_inspect(& self) -> Option<& Vec<Term>> {
    match * self {
      RTerm::App { op: Op::Mul, ref args, .. } => Some(args),
      _ => None,
    }
  }
  /// Returns the kids of a constant multiplication.
  pub fn cmul_inspect(& self) -> Option<(Val, & Term)> {
    match * self {
      RTerm::App { op: Op::CMul, ref args, .. } => {
        if args.len() == 2 {
          if let Some(val) = args[0].val() {
            return Some((val, & args[1]))
          }
        }
        panic!("illegal c_mul application: {}", self)
      },
      _ => None,
    }
  }

  /// Iterator over over all the leafs of a term.
  pub fn leaf_iter(& self) -> LeafIter {
    LeafIter::of_rterm(self)
  }

  /// Iterates over the subterms of a term.
  fn iter<F: FnMut(& RTerm)>(& self, mut f: F) {
    let mut stack = vec![self] ;

    while let Some(term) = stack.pop() {
      f(term) ;
      match term {
        RTerm::App { args, .. } => for term in args {
          stack.push(term)
        },

        RTerm::CArray { term, .. } => stack.push( term.get() ),
        RTerm::DTypSlc { term, .. } => stack.push( term.get() ),

        RTerm::DTypNew { args, .. } => for term in args {
          stack.push(term)
        },

        RTerm::Var(_, _) |
        RTerm::Cst(_) => (),
      }
    }
  }

  /// Type of the term.
  pub fn typ(& self) -> Typ {
    match self {
      RTerm::Var(typ, _) => typ.clone(),
      RTerm::Cst(val) => val.typ(),

      RTerm::CArray { typ, term } => typ::array(
        typ.clone(), term.typ()
      ),

      RTerm::App { typ, .. } => typ.clone(),

      RTerm::DTypSlc { typ, .. } => typ.clone(),
      RTerm::DTypNew { typ, .. } => typ.clone(),
    }
  }

  /// True if the term is zero (integer or real).
  pub fn is_zero(& self) -> bool {
    match ( self.int(), self.real() ) {
      (Some(i), _) => i.is_zero(),
      (_, Some(r)) => r.is_zero(),
      _ => false,
    }
  }

  /// True if the term is one (integer or real).
  pub fn is_one(& self) -> bool {
    use num::One ;
    match ( self.int(), self.real() ) {
      (Some(i), _) => i == Int::one(),
      (_, Some(r)) => r == Rat::one(),
      _ => false,
    }
  }

  /// Write a real term using a special function to write variables.
  pub fn write<W, WriteVar>(
    & self, w: & mut W, write_var: WriteVar
  ) -> IoRes<()>
  where W: Write, WriteVar: Fn(& mut W, VarIdx) -> IoRes<()> {
    let mut stack = vec![
      (vec![self], "", "")
    // ^^^^^^^^^|  ^^| ^^~~~ termination string (written once vector's empty)
    //          |    |~~~~~~ prefix string      (written before next element)
    //          |~~~~~~~~~~~ elements to write
    ] ;

    while let Some( (mut to_do, sep, end) ) = stack.pop() {
      use self::RTerm::* ;

      if let Some(this_term) = to_do.pop() {
        stack.push( (to_do, sep, end) ) ;
        write!(w, "{}", sep) ? ;

        match this_term {
          Var(_, v) => write_var(w, * v) ?,
          Cst(val) => write!(w, "{}", val) ?,

          CArray { term, .. } => {
            write!(w, "((as const {})", this_term.typ()) ? ;
            stack.push( (vec![term], " ", ")") )
          },

          App { op, args, .. } => {
            write!(w, "({}", op) ? ;
            stack.push(
              (args.iter().rev().map(|t| t.get()).collect(), " ", ")")
            )
          },

          DTypSlc { name, term, .. } => {
            write!(w, "({}", name) ? ;
            stack.push( (vec![term], " ", ")") )
          },

          DTypNew { name, args, .. } => if args.is_empty() {
            write!(w, "{}", name) ?
          } else {
            write!(w, "({}", name) ? ;
            stack.push(
              (args.iter().rev().map(|t| t.get()).collect(), " ", ")")
            )
          },
        }

      } else { w.write_all( end.as_bytes() ) ? }
    }

    Ok(())
  }

  /// True if atom `self` implies atom `other` syntactically.
  ///
  /// Returns
  ///
  /// - `None` if no conclusion was reached,
  /// - `Some(Greater)` if `lhs => rhs`,
  /// - `Some(Less)` if `lhs <= rhs`,
  /// - `Some(Equal)` if `lhs` and `rhs` are equivalent.
  ///
  /// So *greater* really means *more generic*.
  ///
  /// See [the module's function][atom implies] for more details and examples.
  ///
  /// [atom implies]: fn.atom_implies.html (atom_implies module-level function)
  pub fn conj_simpl(& self, other: & Self) -> simplify::SimplRes {
    simplify::conj_simpl(& self, & other)
  }

  /// Term evaluation (int).
  pub fn int_eval<E: Evaluator>(
    & self, model: & E
  ) -> Res< Option<Int> > {
    self.eval(model)?.to_int()
  }

  /// Term evaluation (real).
  pub fn real_eval<E: Evaluator>(
    & self, model: & E
  ) -> Res< Option<Rat> > {
    self.eval(model)?.to_real()
  }

  /// Term evaluation (bool).
  pub fn bool_eval<E: Evaluator>(
    & self, model: & E
  ) -> Res< Option<bool> > {
    self.eval(model)?.to_bool()
  }

  /// True if the term has no variables and evaluates to true.
  ///
  /// # Examples
  ///
  /// ```rust
  /// use hoice::term ;
  /// use hoice::term::Op ;
  ///
  /// let term = term::tru() ;
  /// println!("true") ;
  /// assert!( term.is_true() ) ;
  /// let term = term::fls() ;
  /// println!("false") ;
  /// assert!( ! term.is_true() ) ;
  /// let term = term::eq(
  ///   term::int(7), term::int_var(1)
  /// ) ;
  /// println!("7 = v_1") ;
  /// assert!( ! term.is_true() ) ;
  /// let term = term::eq(
  ///   term::int(9), term::int(9)
  /// ) ;
  /// println!("9 = 9") ;
  /// assert!( term.is_true() ) ;
  /// let term = term::eq(
  ///   term::int(1), term::int(9)
  /// ) ;
  /// println!("1 = 9") ;
  /// assert!( ! term.is_true() ) ;
  /// let term = term::le(
  ///   term::app(
  ///     Op::Add, vec![ term::int(3), term::int(4) ]
  ///   ), term::int(9)
  /// ) ;
  /// println!("3 + 4 = 9") ;
  /// assert!( term.is_true() ) ;
  /// ```
  pub fn is_true(& self) -> bool {
    match self.bool_eval( & () ) {
      Ok(Some(b)) => b,
      _ => false,
    }
  }
  
  /// True if the term has no variables and evaluates to true.
  ///
  /// # Examples
  ///
  /// ```rust
  /// use hoice::term ;
  /// use hoice::term::Op ;
  ///
  /// let term = term::tru() ;
  /// println!("true") ;
  /// assert!( ! term.is_false() ) ;
  /// let term = term::fls() ;
  /// println!("false") ;
  /// assert!( term.is_false() ) ;
  /// let term = term::eq(
  ///   term::int(7), term::int_var(1)
  /// ) ;
  /// println!("7 = v_1") ;
  /// assert!( ! term.is_false() ) ;
  /// let term = term::eq(
  ///   term::int(9), term::int(9)
  /// ) ;
  /// println!("9 = 9") ;
  /// assert!( ! term.is_false() ) ;
  /// let term = term::eq(
  ///   term::int(1), term::int(9)
  /// ) ;
  /// println!("1 = 9") ;
  /// assert!( term.is_false() ) ;
  /// let term = term::le(
  ///   term::int(9), term::app(
  ///     Op::Add, vec![ term::int(3), term::int(4) ]
  ///   )
  /// ) ;
  /// println!("9 <= 3 + 4") ;
  /// assert!( term.is_false() ) ;
  /// ```
  pub fn is_false(& self) -> bool {
    match self.bool_eval( & () ) {
      Ok(Some(b)) => ! b,
      _ => false,
    }
  }
  /// Boolean a constant boolean term evaluates to.
  pub fn bool(& self) -> Option<bool> {
    match self.bool_eval( & () ) {
      Ok(Some(b)) => Some(b),
      _ => None
    }
  }

  /// Evaluates a term with an empty model.
  pub fn as_val(& self) -> Val {
    if let Ok(res) = self.eval(& ()) { res } else {
      val::none(self.typ().clone())
    }
  }

  /// Integer a constant integer term evaluates to.
  pub fn int(& self) -> Option<Int> {
    if self.typ() != typ::int() { return None }
    match self.int_eval( & () ) {
      Ok(Some(i)) => Some(i),
      _ => None
    }
  }
  /// Integer a constant integer term evaluates to.
  pub fn real(& self) -> Option<Rat> {
    match self.real_eval( & () ) {
      Ok(Some(r)) => Some(r),
      _ => None
    }
  }

  /// Turns a constant term in a `Val`.
  pub fn val(& self) -> Option<Val> {
    match * self {
      RTerm::Cst(ref val) => Some( val.clone() ),
      _ => None,
    }
  }

  /// Returns a constant arithmetic version of the term if any.
  pub fn arith(& self) -> Option<Term> {
    if let Some(i) = self.int() {
      Some( term::int(i) )
    } else if let Some(r) = self.real() {
      Some( term::real(r) )
    } else {
      None
    }
  }

  /// The kids of this term, if any.
  pub fn kids(& self) -> Option<& [Term]> {
    if let RTerm::App{ ref args, .. } = * self {
      Some(args)
    } else {
      None
    }
  }

  /// Casts a term.
  ///
  /// Only legal if the term's type and the one provided are compatible.
  ///
  /// Returns
  ///
  /// - an error if the types are not compatible
  /// - `None` if the cast didn't do anything
  /// - the new term otherwise
  pub fn cast(& self, to_typ: & Typ) -> Res< Option<Term> > {
    let nu_typ = if let Some(typ) = self.typ().merge(to_typ) {
      if to_typ == & typ { return Ok(None) }
      typ
    } else {
      bail!(
        "types {} and {} are incompatible", self.typ(), to_typ
      )
    } ;

    enum Frame<'a> {
      // Array frame.
      Arr(Typ),
      // Datatype constructor.
      New {
        typ: Typ,
        name: String,
        lft: Vec<Term>,
        rgt: ::std::vec::IntoIter<(Typ, & 'a RTerm)>,
      },
    }

    let mut stack = vec![] ;
    let (mut nu_typ, mut curr) = (nu_typ, self) ;

    'go_down: loop {

      let mut term = match curr {
        RTerm::Var(_, idx) => term::var(* idx, nu_typ),

        RTerm::Cst(val) => if let Ok(val) = val.cast(& nu_typ) {
          factory::cst(val)
        } else {
          return Ok(None)
        },

        RTerm::App { op, args, .. } => term::app( * op, args.clone() ),

        RTerm::CArray { typ, term } => {
          let (src, tgt) = typ.array_inspect().unwrap() ;
          stack.push(
            Frame::Arr( src.clone() )
          ) ;
          nu_typ = tgt.clone() ;
          curr = term.get() ;
          continue 'go_down
        },

        RTerm::DTypNew { typ, name, args } => {
          let mut lft = vec![] ;
          let mut next = None ;
          let mut rgt = vec![] ;

          scoped! {

            let (_, nu_prms) = nu_typ.dtyp_inspect().unwrap() ;
            let (_, prms) = typ.dtyp_inspect().unwrap() ;
            debug_assert_eq! { args.len(), nu_prms.len() }
            debug_assert_eq! { args.len(), prms.len() }
            let mut all = nu_prms.iter().zip(
              prms.iter()
            ).zip( args.iter() ) ;

            while let Some(((nu, typ), arg)) = all.next()  {
              if nu == typ {
                lft.push( arg.clone() )
              } else {
                next = Some(
                  ( arg.get(), nu.clone() )
                )
              }
            }

            for ((nu_typ, _), arg) in all {
              rgt.push( (nu_typ.clone(), arg.get()) )
            }

          }

          if let Some((term, nu)) = next {
            let frame = Frame::New {
              typ: nu_typ, name: name.clone(), lft, rgt: rgt.into_iter()
            } ;
            stack.push(frame) ;
            nu_typ = nu ;
            curr = term ;

            continue 'go_down
          } else {
            term::dtyp_new(nu_typ, name.clone(), lft)
          }
        },

        RTerm::DTypSlc { typ, name, term } => {
          debug_assert_eq! { typ, & nu_typ }
          term::dtyp_slc(
            typ.clone(), name.clone(), term.clone()
          )
        },
      } ;

      'go_up: loop {

        match stack.pop() {
          None => return Ok( Some(term) ),

          Some( Frame::Arr(typ) ) => {
            term = term::cst_array(typ, term) ;
            continue 'go_up
          },

          Some(
            Frame::New { typ, name, mut lft, mut rgt }
          ) => {
            lft.push(term) ;

            if let Some((ty, tm)) = rgt.next() {
              nu_typ = ty ;
              curr = tm ;
              stack.push( Frame::New { typ, name, lft, rgt } ) ;
              continue 'go_down
            } else {
              term = term::dtyp_new(typ, name, lft) ;
              continue 'go_up
            }
          },
        }

      }

    }

  }

  /// Checks whether the term is a relation.
  pub fn is_relation(& self) -> bool {
    match * self {
      RTerm::App { op: Op::Eql, .. } |
      RTerm::App { op: Op::Gt, .. } |
      RTerm::App { op: Op::Ge, .. } |
      RTerm::App { op: Op::Lt, .. } |
      RTerm::App { op: Op::Le, .. } => true,
      RTerm::App { op: Op::Not, ref args, .. } => args[0].is_relation(),
      _ => false,
    }
  }
  /// Checks whether a term is an equality.
  pub fn is_eq(& self) -> bool {
    match * self {
      RTerm::App { op: Op::Eql, .. } => true,
      _ => false,
    }
  }

  /// Folds over a term.
  ///
  /// # Type parameters
  ///
  /// - `Info`: information extracted by the folding process
  /// - `VarF`: will run on variables
  /// - `CstF`: will run on constants
  /// - `AppF`: will run on the result of folding on operator applications
  /// - `ArrF`: will run on the result of folding on arrays
  /// - `NewF`: will run on the result of folding on datatype constructors
  /// - `SlcF`: will run on the result of folding on datatype selectors
  pub fn fold<Info, VarF, CstF, AppF, ArrF, NewF, SlcF>(
    & self,
    varf: VarF, cstf: CstF, appf: AppF, arrf: ArrF, newf: NewF, slcf: SlcF
  ) -> Info
  where
  VarF: FnMut(& Typ, VarIdx) -> Info,
  CstF: FnMut(& Val) -> Info,
  AppF: FnMut(& Typ, Op, Vec<Info>) -> Info,
  ArrF: FnMut(& Typ, Info) -> Info,
  NewF: FnMut(& Typ, & String, Vec<Info>) -> Info,
  SlcF: FnMut(& Typ, & String, Info) -> Info, {
    fold::fold(self, varf, cstf, appf, arrf, newf, slcf)
  }



  /// Folds over a term.
  ///
  /// Early returns **iff** any a call to one of the input functions returns an
  /// error.
  ///
  /// # Type parameters
  ///
  /// - `Info`: information extracted by the folding process
  /// - `VarF`: will run on variables
  /// - `CstF`: will run on constants
  /// - `AppF`: will run on the result of folding on operator applications
  /// - `ArrF`: will run on the result of folding on arrays
  /// - `NewF`: will run on the result of folding on datatype constructors
  /// - `SlcF`: will run on the result of folding on datatype selectors
  pub fn fold_res<Info, VarF, CstF, AppF, ArrF, NewF, SlcF>(
    & self,
    varf: VarF, cstf: CstF, appf: AppF, arrf: ArrF, newf: NewF, slcf: SlcF
  ) -> Res<Info>
  where
  VarF: FnMut(& Typ, VarIdx) -> Res<Info>,
  CstF: FnMut(& Val) -> Res<Info>,
  AppF: FnMut(& Typ, Op, Vec<Info>) -> Res<Info>,
  ArrF: FnMut(& Typ, Info) -> Res<Info>,
  NewF: FnMut(& Typ, & String, Vec<Info>) -> Res<Info>,
  SlcF: FnMut(& Typ, & String, Info) -> Res<Info>, {
    fold::fold_res(self, varf, cstf, appf, arrf, newf, slcf)
  }


  /// Term evaluation.
  ///
  /// # TODO
  ///
  /// - remove recursive call for constant arrays
  pub fn eval<E: Evaluator>(& self, model: & E) -> Res<Val> {
    self.fold_res(
      // Variable evaluation.
      |_, v| if v < model.len() {
        Ok( model.get(v).clone() )
      } else {
        bail!("model is too short ({})", model.len())
      },

      // Constant evaluation.
      |val| Ok( val.clone() ),

      // Operator application evaluation.
      |_, op, values| op.eval(values).chain_err(
        || format!("while evaluating operator `{}`", op)
      ),

      // Constant array evaluation.
      |typ, default| Ok(
        val::array( typ.clone(), default )
      ),

      // Datatype construction.
      |typ, name, values| Ok(
        val::dtyp_new( typ.clone(), name.clone(), values )
      ),

      // Datatype selection.
      |typ, name, value| if ! value.is_known() {
        Ok( val::none( typ.clone() ) )
      } else if let Some(
        (ty, constructor, values)
      ) = value.dtyp_inspect() {
        if let Some((dtyp, _)) = ty.dtyp_inspect() {

          if let Some(selectors) = dtyp.news.get(constructor) {

            let mut res = None ;
            for ((selector, _), value) in selectors.iter().zip(
              values.iter()
            ) {
              if selector == name {
                res = Some( value.clone() )
              }
            }

            if let Some(res) = res {
              Ok(res)
            } else {
              Ok( val::none( typ.clone() ) )
            }

          } else {
            bail!(
              "unknown constructor `{}` for datatype {}",
              conf.bad(constructor), dtyp.name
            )
          }

        } else {
          bail!("inconsistent type {} for value {}", ty, value)
        }
      } else {
        bail!(
          "illegal application of constructor `{}` of `{}` to `{}`",
          conf.bad(& name), typ, value
        )
      }
    )
  }

  /// If the term's an integer constant, returns the value.
  pub fn int_val(& self) -> Option<& Int> {
    if let RTerm::Cst(val) = self {
      if let val::RVal::I(i) = val.get() {
        return Some( i )
      }
    }
    None
  }
  /// If the term's a rational constant, returns the value.
  pub fn real_val(& self) -> Option<& Rat> {
    if let RTerm::Cst(val) = self {
      if let val::RVal::R(r) = val.get() {
        return Some( r )
      }
    }
    None
  }

  /// The highest variable index appearing in the term.
  pub fn highest_var(& self) -> Option<VarIdx> {
    let mut max = None ;

    for var_or_cst in self.leaf_iter() {
      if let Either::Left((_, var_idx)) = var_or_cst {
        max = Some(
          ::std::cmp::max(
            var_idx, max.unwrap_or_else(|| 0.into())
          )
        )
      }
    }

    max
  }

  /// Returns the variable index if the term is a variable.
  pub fn var_idx(& self) -> Option<VarIdx> {
    match * self {
      RTerm::Var(_, i) => Some(i),
      _ => None,
    }
  }

  /// Return true if the term mentions at least one variable from `vars`.
  pub fn mentions_one_of(& self, vars: & VarSet) -> bool {
    for var_or_cst in self.leaf_iter() {
      if let Either::Left((_, var_idx)) = var_or_cst {
        if vars.contains(& var_idx) {
          return true
        }
      }
    }
    false
  }

  /// If the term is a negation, returns what's below the negation.
  pub fn rm_neg(& self) -> Option<Term> {
    match * self {
      RTerm::App { op: Op::Not, ref args, .. } => {
        debug_assert_eq!( args.len(), 1 ) ;
        Some( args[0].clone() )
      },
      _ => None,
    }
  }


  /// Turns a real term in a hashconsed one.
  #[inline]
  pub fn to_hcons(& self) -> Term {
    term( self.clone() )
  }



  /// Variable substitution.
  ///
  /// The `total` flag causes substitution to fail if a variable that's not in
  /// `map`.
  ///
  /// The boolean returned is true if at least on substitution occured.
  pub fn subst_custom<Map: VarIndexed<Term>>(
    & self, map: & Map, total: bool
  ) -> Option<(Term, bool)> {
    let mut changed = false ;

    let res = fold::fold_custom_res(
      self,

      // Variable.
      |typ, var| if let Some(term) = map.var_get(var) {
        debug_assert_eq! { typ, & term.typ() }
        changed = true ;
        Ok(term)
      } else if total {
        Err(())
      } else {
        Ok(
          term::var( var, typ.clone() )
        )
      },

      // Constant.
      |cst| Ok(
        term::cst( cst.clone() )
      ),

      // Operator application.
      |_, op, args| Ok(
        term::app(op, args)
      ),

      // Constant array.
      |typ, default| Ok(
        term::cst_array( typ.clone(), default )
      ),

      // Datatype constructor.
      |typ, name, args| Ok(
        term::dtyp_new(
          typ.clone(), name.clone(), args
        )
      ),

      // Datatype selector.
      |typ, name, term| Ok(
        term::dtyp_slc(
          typ.clone(), name.clone(), term
        )
      ),
    ) ;

    if let Ok(term) = res {
      Some( (term, changed) )
    } else {
      None
    }
  }

  /// Variable substitution.
  ///
  /// Returns the new term and a boolean indicating whether any substitution
  /// occured.
  ///
  /// Used for substitutions in the same clause / predicate scope.
  #[inline]
  pub fn subst<Map: VarIndexed<Term>>(
    & self, map: & Map
  ) -> (Term, bool) {
    self.subst_custom(map, false).expect("total substitution can't fail")
  }

  /// Fixed-point (partial) variable substitution.
  ///
  /// Returns the new term and a boolean indicating whether any substitution
  /// occured.
  pub fn subst_fp<Map: VarIndexed<Term>>(
    & self, map: & Map
  ) -> (Term, bool) {
    let (mut term, mut changed) = self.subst(map) ;
    while changed {
      let (new_term, new_changed) = term.subst(map) ;
      term = new_term ;
      changed = new_changed
    }
    (term, changed)
  }

  /// Total variable substition, returns `None` if there was a variable in the
  /// term that was not in the map.
  ///
  /// Returns the new term and a boolean indicating whether any substitution
  /// occsured.
  ///
  /// Used for substitutions between different same clause / predicate scopes.
  pub fn subst_total<Map: VarIndexed<Term>>(
    & self, map: & Map
  ) -> Option< (Term, bool) > {
    self.subst_custom(map, true)
  }


  /// Tries to turn a term into a substitution.
  ///
  /// Works only on equalities.
  ///
  /// # Examples
  ///
  /// ```rust
  /// use hoice::term ;
  ///
  /// let bv0 = term::bool_var(0) ;
  /// let bv1 = term::bool_var(1) ;
  /// let bv2 = term::bool_var(2) ;
  /// let rhs = term::or(vec![bv1, bv2]) ;
  /// let term = term::eq(bv0, rhs.clone()) ;
  /// debug_assert_eq! { term.as_subst(), Some((0.into(), rhs)) }
  /// ```
  pub fn as_subst(& self) -> Option<(VarIdx, Term)> {
    if let Some(kids) = self.eq_inspect() {
      debug_assert_eq! { kids.len(), 2 }
      let (lhs, rhs) = (& kids[0], & kids[1]) ;

      if let Some(var_idx) = lhs.var_idx() {
        return Some((var_idx, rhs.clone()))
      } else if let Some(var_idx) = rhs.var_idx() {
        return Some((var_idx, lhs.clone()))
      }

      if lhs.typ().is_arith() {
        debug_assert! { rhs.is_zero() }

        let lhs = if let Some((_, term)) = lhs.cmul_inspect() {
          term
        } else { lhs } ;

        let mut add = vec![] ;
        let mut var = None ;
        let mut negated = false ;

        if let Some(kids) = lhs.add_inspect() {
          for kid in kids {
            if var.is_some() {
              add.push(kid.clone()) ;
              continue
            }
            if let Some(var_index) = kid.var_idx() {
              debug_assert! { var.is_none() }
              var = Some(var_index) ;
              continue
            } else if let Some((val, term)) = kid.cmul_inspect() {
              if let Some(var_index) = term.var_idx() {
                if val.is_one() {
                  var = Some(var_index) ;
                  continue
                } else if val.is_minus_one() {
                  var = Some(var_index) ;
                  negated = true ;
                  continue
                }
              }
            }
            add.push(kid.clone())
          }

          if let Some(var) = var {
            let mut sum = term::add(add) ;
            if ! negated { sum = term::u_minus(sum) }
            Some((var, sum))
          } else {
            None
          }
        } else {
          None
        }
      } else {
        None
      }

    } else {
      None
    }
  }



  /// Attempts to invert a term from a variable.
  pub fn invert_var(& self, var: VarIdx, typ: Typ) -> Option<(VarIdx, Term)> {
    self.invert( term::var(var, typ) )
  }

  /// Attempts to invert a term.
  ///
  /// More precisely, if the term only mentions one variable `v`, attempts to
  /// find a `f` that's a solution of `var = term <=> v = f(var)`.
  ///
  /// Currently, only works when `v` appears exactly once. That is, it will
  /// fail on `var = 3.v + 7.v` for instance. (This would be fine if
  /// normalization handled this kind cases though.)
  ///
  /// Also, only works when all operators are binary (expect for unary minus).
  ///
  /// # Examples
  ///
  /// ```rust
  /// use hoice::term ;
  ///
  /// let term = term::u_minus( term::int_var(0) ) ;
  /// println!("{}", term) ;
  /// assert_eq!{
  ///   term.invert( term::int_var(1) ),
  ///   Some( (0.into(), term::u_minus( term::int_var(1) ) ) )
  /// }
  /// let term = term::sub( vec![ term::int_var(0), term::int(7) ] ) ;
  /// println!("{}", term) ;
  /// assert_eq!{
  ///   term.invert( term::int_var(1) ),
  ///   Some( (0.into(), term::add( vec![ term::int_var(1), term::int(7) ] ) ) )
  /// }
  /// let term = term::add( vec![ term::int(7), term::int_var(0) ] ) ;
  /// println!("{}", term) ;
  /// assert_eq!{
  ///   term.invert( term::int_var(1) ),
  ///   Some(
  ///     (0.into(), term::sub( vec![ term::int_var(1), term::int(7) ] ) )
  ///   )
  /// }
  /// ```
  pub fn invert(& self, term: Term) -> Option<(VarIdx, Term)> {
    let mut solution = term ;
    let mut term = self ;

    loop {
      // println!("inverting {}", term) ;
      match * term {
        RTerm::App { op, ref args, .. } => {
          let (po, symmetric) = match op {
            Op::Add => (Op::Sub, true),
            Op::Sub => {
              if args.len() == 1 {
                solution = term::u_minus( solution ) ;
                term = & args[0] ;
                continue
              } else if args.len() == 2 {
                if args[0].val().is_some() {
                  solution = term::sub(
                    vec![ args[0].clone(), solution ]
                  ) ;
                  term = & args[1] ;
                  continue
                } else if args[1].val().is_some() {
                  solution = term::add(
                    vec![ args[1].clone(), solution ]
                  ) ;
                  term = & args[0] ;
                  continue
                }
              }
              return None
            },
            Op::IDiv => return None,
            Op::CMul => {
              if args.len() == 2 {
                if let Some(val) = args[0].val() {
                  if val.minus().expect(
                    "illegal c_mul application found in `invert`"
                  ).is_one() {
                    solution = term::u_minus(solution) ;
                    term = & args[1] ;
                    continue
                  } else {
                    return None
                  }
                }
              }

              panic!("illegal c_mul application found in `invert`")
            },
            // Op::Div => (Op::Mul, false),
            // Op::Mul => (Op::Div, true),
            Op::ToReal => {
              solution = term::to_int(solution) ;
              term = & args[0] ;
              continue
            },
            Op::ToInt => {
              solution = term::to_real(solution) ;
              term = & args[0] ;
              continue
            },
            _ => return None,
          } ;
          if args.len() != 2 { return None }

          if let Some(arith) = args[0].arith() {
            if symmetric {
              solution = term::app( po, vec![ solution, arith ] )
            } else {
              solution = term::app( op, vec![ arith, solution ] )
            }
            term = & args[1]
          } else if let Some(arith) = args[1].arith() {
            solution = term::app( po, vec![ solution, arith ] ) ;
            term = & args[0]
          } else {
            return None
          }
        },

        RTerm::Var(_, v) => return Some((v, solution)),

        RTerm::Cst(_)         |
        RTerm::CArray  { .. } |
        RTerm::DTypNew { .. } |
        RTerm::DTypSlc { .. } => return None,
      }
    }
  }

}


impl_fmt!{
  RTerm(self, fmt) {
    let mut buf = Vec::with_capacity(250) ;
    self.write(& mut buf, |w, var| var.default_write(w)).expect(
      "fatal error during real term pretty printing"
    ) ;
    let s = ::std::str::from_utf8(& buf).expect(
      "fatal error during real term pretty printing"
    ) ;
    write!(fmt, "{}", s)
  }
}
impl<'a> PebcakFmt<'a> for RTerm {
  type Info = & 'a VarMap< ::instance::info::VarInfo > ;
  fn pebcak_err(& self) -> ErrorKind {
    "during term pebcak formatting".into()
  }
  fn pebcak_io_fmt<W: Write>(
    & self, w: & mut W, vars: & 'a VarMap< ::instance::info::VarInfo >
  ) -> IoRes<()> {
    self.write(
      w, |w, var| w.write_all( vars[var].as_bytes() )
    )
  }
}

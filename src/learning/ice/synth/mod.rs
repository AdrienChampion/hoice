//! Qualifier synthesis modulo theory.
//!
//! # TO DO
//!
//! - document the workflow
//! - factor code between the synthesizers (`eq_synth` etc.)

use common::* ;

#[macro_use]
pub mod helpers ;
pub mod int ;
pub mod real ;
pub mod adt ;

pub type TermVals = TermMap<Val> ;

/// A theory synthesizer.
///
/// A `TheoSynth` synthezises qualifiers given some arguments for a predicate
/// and some additional term/value pair, that typically come from other
/// theories. These pairs are the result of projecting/casting/... an argument
/// of a different theory to this one.
///
/// It is iterable. Each version generates qualifiers more complex than the
/// previous one, making synthesis more expressive with each call to `next`.
pub trait TheoSynth {
  /// Type of values supported by this synthesizer.
  fn typ(& self) -> & Typ ;
  /// Returns `true` if the synthesizer is done (will not produce new
  /// qualifiers).
  fn is_done(& self) -> bool ;
  /// Restarts the synthesizer.
  fn restart(& mut self) ;
  /// Increments the synthesizer.
  fn increment(& mut self) ;
  /// Synthesizes qualifiers.
  fn synth<F>(& mut self, F, & VarVals, & mut TermVals, & Profiler) -> Res<bool>
  where F: FnMut(Term) -> Res<bool> ;
  /// Generates some [`TermVal`][term val]s for some other type.
  ///
  /// Adds them to the input term to value map.
  ///
  /// [term val]: struct.TermVal.html
  /// (TermVal struct)
  fn project(& self, & VarVals, & Typ, & mut TermVals) -> Res<()> ;
}

use self::int::IntSynth ;
use self::real::RealSynth ;
use self::adt::AdtSynth ;

/// Manages theory synthesizers.
pub struct SynthSys {
  int: Option<IntSynth>,
  real: Option<RealSynth>,
  adt: Vec<AdtSynth>,
  cross_synth: TermMap<Val>,
}
impl SynthSys {
  /// Constructor.
  pub fn new(sig: & Sig) -> Self {
    let mut int = None ;
    let mut real = None ;

    macro_rules! set {
      (int) => (
        if int.is_none() {
          int = Some( IntSynth::new() )
        }
      ) ;
      (real) => (
        if real.is_none() {
          real = Some( RealSynth::new() )
        }
      ) ;
    }

    let mut adt: Vec<AdtSynth> = Vec::new() ;
    for typ in sig {
      match ** typ {
        typ::RTyp::Int => set!(int),
        typ::RTyp::Real => set!(real),

        typ::RTyp::DTyp { .. } => if adt.iter().all(
          |adt| adt.typ() != typ
        ) {
          let synth = AdtSynth::new( typ.clone() ) ;
          // println!("creating synth for {}", synth.typ()) ;
          // println!("  from_typ:") ;
          // for fun in & synth.funs.from_typ {
          //   println!("  - {}", fun.name)
          // }
          // println!("  to_typ:") ;
          // for fun in & synth.funs.to_typ {
          //   println!("  - {}", fun.name)
          // }
          // println!("  from_to_typ:") ;
          // for fun in & synth.funs.from_to_typ {
          //   println!("  - {}", fun.name)
          // }
          if synth.can_project_to_int() { set!(int) }
          if synth.can_project_to_real() { set!(real) }
          adt.push(synth)
        },

        typ::RTyp::Bool |
        typ::RTyp::Array { .. } |
        typ::RTyp::Unk => (),
      }
    }

    SynthSys {
      int, real, adt, cross_synth: TermMap::new(),
    }
  }

  /// True if all synthesizers are done.
  pub fn is_done(& self) -> bool {
    self.int.as_ref().map(|i| i.is_done()).unwrap_or(true) &&
    self.real.as_ref().map(|r| r.is_done()).unwrap_or(true) &&
    self.adt.iter().all(
      |a| a.is_done()
    )
  }

  /// Increments all synthesizers.
  pub fn increment(& mut self) {
    if let Some(i) = self.int.as_mut() { i.increment() }
    if let Some(r) = self.real.as_mut() { r.increment() }
    for a in & mut self.adt {
      a.increment()
    }
  }

  /// Restarts all synthesizers.
  pub fn restart(& mut self) {
    if let Some(i) = self.int.as_mut() { i.restart() }
    if let Some(r) = self.real.as_mut() { r.restart() }
    for a in & mut self.adt {
      a.restart()
    }
  }


  /// Synthesizes qualifiers for a sample, stops if input function returns
  /// `true`.
  ///
  /// Returns `true` iff `f` returned true at some point.
  pub fn sample_synth<F>(
    & mut self, sample: & VarVals, mut f: F, _profiler: & Profiler
  ) -> Res<bool>
  where F: FnMut(Term) -> Res<bool> {

    if let Some(int_synth) = self.int.as_mut() {
      if ! int_synth.is_done() {
        self.cross_synth.clear() ;

        if let Some(real_synth) = self.real.as_mut() {
          profile!{
            |_profiler| tick "learning", "qual", "synthesis", "int project"
          }
          let res = real_synth.project(
            sample, int_synth.typ(), & mut self.cross_synth
          ) ;
          profile!{
            |_profiler| mark "learning", "qual", "synthesis", "int project"
          }
          res ?
        }
        for adt_synth in & mut self.adt {
          profile!{
            |_profiler| tick "learning", "qual", "synthesis", "adt project"
          }
          let res = adt_synth.project(
            sample, int_synth.typ(), & mut self.cross_synth
          ) ;
          profile!{
            |_profiler| mark "learning", "qual", "synthesis", "adt project"
          }
          res ?
        }

        profile!{ |_profiler| tick "learning", "qual", "synthesis", "int" }
        let done = int_synth.synth(
          & mut f, sample, & mut self.cross_synth, _profiler
        ) ;
        profile!{ |_profiler| mark "learning", "qual", "synthesis", "int" }
        if done ? { return Ok(true) }
      }
    }

    if let Some(real_synth) = self.real.as_mut() {
      if ! real_synth.is_done() {
        self.cross_synth.clear() ;

        if let Some(int_synth) = self.int.as_mut() {
          profile! (
            |_profiler| wrap {
              int_synth.project(
                sample, real_synth.typ(), & mut self.cross_synth
              )
            } "learning", "qual", "synthesis", "real project"
          ) ?
        }
        for adt_synth in & mut self.adt {
          profile!{
            |_profiler| tick "learning", "qual", "synthesis", "adt project"
          }
          let res = adt_synth.project(
            sample, real_synth.typ(), & mut self.cross_synth
          ) ;
          profile!{
            |_profiler| mark "learning", "qual", "synthesis", "adt project"
          }
          res ?
        }

        profile!{ |_profiler| tick "learning", "qual", "synthesis", "real" }
        let done = real_synth.synth(
          & mut f, sample, & mut self.cross_synth, _profiler
        ) ;
        profile!{ |_profiler| mark "learning", "qual", "synthesis", "real" }
        if done ? { return Ok(true) }
      }
    }

    for adt_synth in & mut self.adt {
      if ! adt_synth.is_done() {
        self.cross_synth.clear() ;

        if let Some(int_synth) = self.int.as_mut() {
          profile! (
            |_profiler| wrap {
              int_synth.project(
                sample, adt_synth.typ(), & mut self.cross_synth
              )
            } "learning", "qual", "synthesis", "real project"
          ) ?
        }
        if let Some(real_synth) = self.real.as_mut() {
          profile!{
            |_profiler| tick "learning", "qual", "synthesis", "int project"
          }
          let res = real_synth.project(
            sample, adt_synth.typ(), & mut self.cross_synth
          ) ;
          profile!{
            |_profiler| mark "learning", "qual", "synthesis", "int project"
          }
          res ?
        }

        profile!{ |_profiler| tick "learning", "qual", "synthesis", "adt" }
        let done = adt_synth.synth(
          & mut f, sample, & mut self.cross_synth, _profiler
        ) ;
        profile!{ |_profiler| mark "learning", "qual", "synthesis", "adt" }
        if done ? { return Ok(true) }
      }
    }

    Ok(false)
  }
}

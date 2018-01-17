//! Types related to qualifiers.
//!
//! A qualifier is essentially a signature and a term. The idea is that, given
//! some sample (input values) for a predicate, we want to to find all the
//! combinations of values that make sense for this qualifier.
//!
//! We also want to avoid storing the same qualifier more than once. For
//! instance:
//!
//! ```smt
//! (define-fun qual      ((v_1 Int) (v_2 Bool))
//!   (ite v_2 (> v_1 0) (= v_1 0))
//! )
//! (define-fun same_qual ((v_1 Bool) (v_2 Int))
//!   (ite v_1 (> v_2 0) (= v_2 0))
//! )
//! ```
//!
//! Hence the signature of a qualifier is sorted by the order of apparition of
//! each variable in the qualifier. For instance, say we want to have the
//! following qualifier
//!
//! ```smt
//! (define-fun qual ((v_1 Int) (v_2 Bool) (v_3 Int))
//!   (ite v_2 (= v_3 (+ v_1 7)) (= v_1 0))
//! )
//! ```
//!
//! Then, the signature is re-ordered as `v_2`, `v_3`, `v_1`. The qualifier
//! becomes
//!
//! ```smt
//! (define-fun qual ((v_1 Bool) (v_2 Int) (v_3 Int))
//!   (ite v_1 (= v_2 (+ v_3 7)) (= v_3 0))
//! )
//! ```
//!
//! This guarantees that two qualifiers coming from the same term modulo
//! alpha-renaming will yield the same qualifier. Terms are currently not in
//! normal form though, so the same is not true for semantically-equivalent
//! terms.
//!
//! **Remark about equality.** One might think that two qualifiers with the
//! same term have to be the same qualifier. This is not true because of
//! polymorphism:
//!
//! ```smt
//! (define-fun qual ((v_1 Int) (v_2 Int))
//!   (= v_1 v_2)
//! )
//! (define-fun other_qual ((v_1 Bool) (v_2 Bool))
//!   (= v_1 v_2)
//! )
//! ```
//!
//! More precisely, this is currently not true because qualifiers cannot be
//! polymorphic.

use hashconsing::* ;


use common::* ;
use instance::info::VarInfo ;



// /// Arguments for qualifiers. Different from predicate arguments.
// #[derive(PartialEq, Eq, Hash)]
// pub struct RQArgs {
//   /// Actual arguments.
//   pub args: VarMap<Val>,
// }
// impl Deref for RQArgs {
//   type Target = VarMap<Val> ;
//   fn deref(& self) -> & VarMap<Val> {
//     & self.args
//   }
// }
/// Hashconsed version of `RQArgs`.
pub type QArgs = HConsed< VarMap<Val> > ;

// /// Type of the `QArgs` factory.
type Factory = HashConsign<VarMap<Typ>> ;


/// Signature of a qualifier.
pub type QSig = VarMap<Typ> ;



/// Information about a qualifier.
pub struct QInfo {
  /// Indicates whether the qualifier has been evaluated at least once.
  pub is_new: bool,
  /// Predicates the qualifier was created for.
  pub preds: PrdSet,
}
impl QInfo {
  /// Constructor.
  pub fn new(pred: PrdIdx) -> Self {
    let mut preds = PrdSet::with_capacity(3) ;
    preds.insert(pred) ;
    QInfo {
      is_new: true,
      preds,
    }
  }
}



/// Type of a signature transformation.
pub type Transform = VarMap<VarIdx> ;

/// Some transformations, either complete or partial.
///
/// - *complete* means that all possible transformations have been generated,
/// - *partial* means that only specific transformations are considered: the
///   ones given when inserting a term.
///
/// The goal is to avoid the combinatorial blow-up that can happen when
/// generating all transformations.
pub enum Transforms {
  /// Complete, contains all the transformations.
  Complete( Vec<Transform> ),
  /// Partial, on-demand transformations.
  Partial( Vec<Transform> ),
}
impl ::std::ops::Deref for Transforms {
  type Target = Vec<Transform> ;
  fn deref(& self) -> & Self::Target {
    match * self {
      Transforms::Complete(ref vec) => vec,
      Transforms::Partial(ref vec) => vec,
    }
  }
}
impl Transforms {
  /// Inserts a transformation in `Partial`, does nothing on `Complete`.
  pub fn insert(& mut self, transform: Transform) {
    match * self {
      Transforms::Partial(ref mut ts) => if ts.iter().all(
        |t| t != & transform
      ) {
        ts.push(transform)
      },
      Transforms::Complete(_) => (),
    }
  }
}


/// For a specific qualifier signature, maps a predicate signature to all the
/// predicate var to qualifier var mappings that work type-wise.
pub struct SigTransforms {
  /// Actual map.
  pub map: HashMap< VarMap<Typ>, Transforms >,
}

impl ::std::ops::Deref for SigTransforms {
  type Target = HashMap< VarMap<Typ>, Transforms > ;
  fn deref(& self) -> & Self::Target {
    & self.map
  }
}
impl ::std::ops::DerefMut for SigTransforms {
  fn deref_mut(& mut self) -> & mut Self::Target {
    & mut self.map
  }
}

impl SigTransforms {
  /// Checks consistency.
  #[inline]
  #[cfg( not(debug_assertions) )]
  pub fn check(& self) -> Res<()> { Ok(()) }
  #[cfg(debug_assertions)]
  pub fn check(& self) -> Res<()> {
    let mut prev = None ;
    for (sig, transs) in & self.map {
      for trans in transs.iter() {
        if prev.is_none() { prev = Some( trans.len() ) }
        if prev != Some( trans.len() ) {
          bail!(
            "sig transform is inconsistent: \
            non-unique transformation arity"
          )
        }
        for idx in trans {
          if * idx >= sig.len() {
            bail!(
              "sig transform is inconsistent: \
              co-domain inconsistent with signature"
            )
          }
        }
      }
    }
    if prev == Some(0) {
      bail!(
        "illegal sig transform: \
        empty transformations"
      )
    }
    Ok(())
  }


  /// Constructor.
  pub fn new(
    preds: & PrdMap<::instance::info::PrdInfo>,
    qual_sig: & QSig,
  ) -> Self {
    let mut map = HashMap::with_capacity( preds.len() ) ;
    // The stack is explained below.
    let mut stack = Vec::with_capacity(17) ;
    'all_preds: for info in preds {
      // Skip if already known.
      if map.contains_key(& info.sig) { continue }

      // Anticipate blowup.
      macro_rules! partial_and_continue {
        () => ({
          let prev = map.insert(
            info.sig.clone(),
            Transforms::Partial(Vec::with_capacity(7))
          ) ;
          debug_assert! { prev.is_none() }
          continue 'all_preds
        })
      }
      if ! conf.ice.complete {
        partial_and_continue!()
      }
      let mut res: u64 = 1 ;
      for typ in qual_sig {
        let mut mul = 0 ;
        for t in & info.sig {
          if t == typ { mul += 1 }
        }
        if let Some(r) = res.checked_mul(mul) {
          res = r
        } else {
          partial_and_continue!()
        }
      }
      if res > 100 {
        partial_and_continue!()
      }

      // println!("{}({})", info, info.sig.len()) ;

      let mut res = Vec::with_capacity(103) ;
      let mut p_sig = info.sig.index_iter() ;
      let mut q_sig = qual_sig.index_iter() ;

      // This is a bit technical.
      stack.push( (
        // We're going to iterate and backtrack on `q_sig` to construct all
        // possible arrangements of variables from `info` that work type-wise.
        q_sig,
        // The map we're currently constructing. Will be cloned when going down
        // (so that we can backtrack later).
        VarMap::with_capacity( qual_sig.len() ),
        // This will store the variables used in the codomain of the map being
        // constructed (previous element of the tuple) so that we can avoid
        // them.
        VarSet::with_capacity( qual_sig.len() ),
        // This is an iterator on the predicate variables that can be
        // considered at the current decision level. Each time we backtrack to
        // this level, we consume it more and more.
        p_sig,
      ) ) ;

      'build_maps: while let Some(
        (mut q_sig, mut map, mut used, mut p_sig)
      ) = stack.pop() {

        // Remember this to memorize current state later.
        let old_q_sig = q_sig.clone() ;

        // What's the next type we want?
        if let Some((_var, q_typ)) = q_sig.next() {
          // println!("  q_{}: {}", var.default_str(), q_typ) ;

          // Find a variable in `p_sig` that has this type and is unknown.
          while let Some((idx, p_typ)) = p_sig.next() {
            // println!("    q_{}: {}", idx.default_str(), p_typ) ;
            // If not the right type...
            if p_typ != q_typ
            // ...or already in use...
            || used.contains(& idx) {
              // ...then skip.
              continue
            }
            // println!("      going down") ;

            // Otherwise, memorize current state: only difference is that we
            // discarded everything in `p_sig` until and including `idx`. This
            // is what we will backtrack to later.
            stack.push(
              ( old_q_sig.clone(), map.clone(), used.clone(), p_sig )
            ) ;
            // ...update map...
            map.push(idx) ;
            // ...update used predicate variables...
            let is_new = used.insert(idx) ;
            debug_assert! { is_new }
            // ...keep going: work on next type in `q_sig`.
            stack.push(
              ( q_sig, map, used, info.sig.index_iter() )
              // Create a new     ^^^^^^^^^^^^^^^^^^^^^
              // iterator on predicate variables for the new decision level.
            ) ;
            continue 'build_maps
          }

          // println!("    nothing works, go up") ;
          // If we're here it's because there nothing in `p_sig` that works
          // anymore. We discard the current decision level and backtrack.
          continue 'build_maps

        } else {
          // println!("  complete, go up") ;
          // No next type, current mapping is complete. Push and keep going.
          if res.iter().any(
            |m| m == & map
          ) {
            panic!("stuttering")
          }
          res.push(map)
        }
      }

      if ! res.is_empty() {
        res.shrink_to_fit() ;
        let prev = map.insert(
          info.sig.clone(), Transforms::Complete(res)
        ) ;
        debug_assert! { prev.is_none() }
      }

    }

    map.shrink_to_fit() ;
    SigTransforms { map }
  }
}


// /// For a specific qualifier signature, maps samples (predicate input values)
// /// to `QArgs`.
// pub struct SampleMap {
//   /// Actual map.
//   map: HConMap< HSample, Option< HConSet<QArgs> > >,
//   /// Signature transformations.
//   transforms: SigTransforms,
// }
// impl SampleMap {
//   /// Constructor.
//   pub fn new(
//     preds: & PrdMap<::instance::info::PrdInfo>, qual_sig: & QSig
//   ) -> Self {
//     let transforms = SigTransforms::new(preds, qual_sig) ;
//     SampleMap {
//       map: HConMap::with_capacity(1001), transforms
//     }
//   }

//   /// Returns the `QArgs` corresponding to a sample, no cache.
//   fn extract(
//     transforms: & SigTransforms, factory: & mut Factory,
//     sample: & HSample, sample_sig: & VarMap<Typ>
//   ) -> Option< HConSet<QArgs> > {
//     if let Some(trans) = transforms.get(sample_sig) {
//       debug_assert! { ! trans.is_empty() }
//       let mut res: HConSet<QArgs> = HConSet::with_capacity( trans.len() ) ;
//       for map in trans {
//         let mut qargs = VarMap::with_capacity( map.len() ) ;
//         for p_idx in map {
//           qargs.push( sample[* p_idx].clone() )
//         }
//         let qargs = factory.mk(qargs) ;
//         res.insert(qargs) ;
//       }
//       Some(res)
//     } else {
//       None
//     }
//   }

//   /// Gets the `QArgs` corresponding to a sample.
//   ///
//   /// The `factory` is required to create `QArgs`.
//   ///
//   /// The `sample_sig` is required to know which `QArgs` to create. It's the
//   /// signature of the predicate the sample comes from.
//   pub fn get<'a>(
//     & mut self, factory: & mut Factory,
//     sample: & HSample, sample_sig: & VarMap<Typ>
//   ) -> Option< & HConSet<QArgs> > {
//     use std::collections::hash_map::Entry ;
//     match self.map.entry( sample.clone() ) {
//       Entry::Occupied(entry) => entry.into_mut().as_ref(),
//       Entry::Vacant(entry) => entry.insert(
//         Self::extract(& self.transforms, factory, sample, sample_sig)
//       ).as_ref(),
//     }
//   }
// }


/// Stores qualifiers that have the same signature.
pub struct QualClass {
  /// Signature transformations.
  pub transforms: SigTransforms,
  /// Qualifiers: map from terms to their info.
  pub quals: HConMap<Term, QInfo>,
}

impl QualClass {
  /// Checks consistency.
  #[inline]
  #[cfg( not(debug_assertions) )]
  pub fn check(& self) -> Res<()> { Ok(()) }
  #[cfg(debug_assertions)]
  pub fn check(& self) -> Res<()> {
    if self.transforms.is_empty() {
      bail!("illegal qual class: no transformations")
    }
    self.transforms.check() ? ;
    for transs in self.transforms.values() {
      for trans in transs.iter() {
        'check_quals: for qual in self.quals.keys() {
          for var in term::vars(qual) {
            if * var >= trans.len() {
              bail!(
                "illegal qual class: \
                qual {} goes above the max variable `v_{}`",
                qual, trans.len() - 1
              )
            }
          }

        }
      }
    }
    Ok(())
  }


  /// Constructor.
  pub fn new(transforms: SigTransforms, qual_capa: usize) -> Option<Self> {
    if transforms.is_empty() {
      None
    } else {
      Some(
        QualClass {
          transforms,
          quals: HConMap::with_capacity(qual_capa)
        }
      )
    }
  }

  /// Inserts a new term in the class.
  ///
  /// Tricky arguments:
  ///
  /// - `pred_sig`: (predicate) signature this new term was extracted from,
  /// - `hint_map`: map from term's variables to predicate's.
  ///
  /// These two hints are only useful when the transforms for `pred_sig` are
  /// stored in a partial manner. In this case, `hint_map` is added to the list
  /// of partial maps.
  pub fn insert(
    & mut self, term: Term, pred: PrdIdx,
    pred_sig: & VarMap<Typ>, hint_map: VarMap<VarIdx>
  ) -> bool {
    use std::collections::hash_map::Entry ;
    if let Some(transforms) = self.transforms.get_mut(pred_sig) {
      transforms.insert(hint_map)
    } else {
      panic!("unknown predicate signature {}", pred_sig)
    }
    if ! self.quals.contains_key( & term::not( term.clone() ) ) {
      match self.quals.entry(term) {
        Entry::Occupied(entry) => {
          let entry = entry.into_mut() ;
          let is_new = entry.preds.insert(pred) ;
          if is_new {
            // Qualifier is new **for this predicate**.
            entry.is_new = true
          }
          // But qualifier is not new in general.
          false
        },
        Entry::Vacant(entry) => {
          entry.insert( QInfo::new(pred) ) ;
          true
        },
      }
    } else {
      false
    }
  }
}



/// Specializes the notion of qualifier defined in this module to a
/// version tailored for one predicate.
///
/// The same qualifier will in general be tailored in different ways for the
/// same predicate.
///
/// This type is in fact a temporary structure created internally by
/// `Qualifiers` for its [`maximize`][quals max] function.
///
/// [quals max]: struct.Qualifiers.html#methods.maximize (Qualifiers' maximize function)
pub struct Qual<'a> {
  /// The qualifier.
  pub qual: & 'a Term,
  // cache: & 'a mut EvalCache,
  /// Maps qualifier variables to predicate variables.
  pub map: & 'a VarMap<VarIdx>,
}
impl<'a> Qual<'a> {
  /// Checks consistency.
  #[inline]
  #[cfg( not(debug_assertions) )]
  pub fn check(& self) -> Res<()> { Ok(()) }
  #[cfg(debug_assertions)]
  pub fn check(& self) -> Res<()> {
    for var in term::vars(self.qual) {
      if * var >= self.map.len() {
        bail!(
          "illegal qualifier: {} goes above max variable `v_{}`",
          self.qual, self.map.len()
        )
      }
    }
    Ok(())
  }

  /// Evaluates this qualifier.
  pub fn bool_eval<E>(& mut self, vals: & E) -> Res<Option<bool>>
  where E: term::Evaluator {
    self.qual.bool_eval( & (self.map, vals) )
  }

  /// Extracts the term corresponding to this specialized qualifier.
  pub fn to_term(& self) -> Term {
    if let Some((term, changed)) = self.qual.subst_total(self.map) {
      debug_assert! { changed }
      term
    } else {
      panic!("bug in new qualifier system, could not retrieve term")
    }
  }
}



/// Stores qualifiers and a lot of stuff for (cached) evaluation.
///
/// # TODO
///
/// - `classes` is probably not deterministic right now: fix
pub struct Qualifiers {
  // /// `QArgs` factory.
  factory: Factory,
  /// Map from **qualifier** signatures to qualifier classes.
  pub classes: HConMap< HConsed<VarMap<Typ>>, QualClass >,
  /// Arc to the instance.
  pub instance: Arc<Instance>,
}

impl Qualifiers {
  /// Checks consistency.
  #[inline]
  #[cfg( not(debug_assertions) )]
  pub fn check(& self) -> Res<()> { Ok(()) }
  #[cfg(debug_assertions)]
  pub fn check(& self) -> Res<()> {
    for (sig, class) in self.classes.iter() {
      class.check() ? ;
      for transs in class.transforms.values() {
        for trans in transs.iter() {
          if trans.len() != sig.len() {
            bail!(
              "inconsistent quals: \
              found a transformation of arity {} but \
              qual signature arity is {}", trans.len(), sig.len()
            )
          }
        }
      }
    }
    Ok(())
  }

  /// Constructor.
  ///
  /// - `factory_capa`: size of the `QArgs` factory
  /// - `class_capa`: space allocated for qualifier classes `QualClass`
  pub fn new(
    // factory_capa: usize,
    // class_capa: usize,
    instance: Arc<Instance>,
  ) -> Res<Self> {
    let class_capa = 13 ;
    let mut quals = Qualifiers {
      factory: Factory::with_capacity(17),
      classes: HConMap::with_capacity(class_capa),
      instance: instance.clone(),
    } ;

    instance.qualifiers(& mut quals) ;

    quals.check().chain_err( || "after creation" ) ? ;

    Ok(quals)
  }


  /// Number of qualifiers.
  pub fn qual_count(& self) -> usize {
    let mut count = 0 ;
    for class in self.classes.values() {
      count += class.quals.len()
    }
    count
  }



  /// Returns the qualifier that maximized the input criterion in a non-zero
  /// fashion, if any. Early-returns if the criterion is `1.0` at some point.
  ///
  /// The criterion `Crit` can return some info, and the info of the best
  /// qualifier will be returned.
  pub fn maximize<Crit>(
    & mut self, pred: PrdIdx, mut crit: Crit, new_only: bool
  ) -> Res< Option<(Term, f64)> >
  where Crit: FnMut( & mut Qual ) -> Res<Option<f64>> {
    let sig = & self.instance.preds()[pred].sig ;
    let mut prev = None ;
    for class in self.classes.values_mut() {
      if let Some(maps) = class.transforms.get(sig) {
        let quals = & mut class.quals ;
        'all_quals: for (qual, info) in quals.iter_mut() {
          
          if conf.ice.qual_bias && ! info.preds.contains(& pred)
          || new_only && ! info.is_new {
            continue 'all_quals
          }
          info.is_new = false ;

          'all_maps: for map in maps.iter() {
            let qual = & mut Qual { qual, map } ;
            qual.check() ? ;
            // println!("- {}", qual.to_term()) ;
            let res = if let Some(res) = crit(qual).chain_err(
              || "during criterion evaluation"
            ) ? {
              res
            } else {
              // println!("  none") ;
              continue 'all_maps
            } ;

            // println!("  {}", res) ;

            if res == 0.0 {
              continue 'all_maps
            } else if res == 1.0 {
              return Ok(
                Some(
                  (qual.to_term(), res)
                )
              )
            }
            prev = if let Some((p_term, p_res)) = prev {
              if p_res < res {
                Some( (qual.to_term(), res) )
              } else {
                Some( (p_term, p_res) )
              }
            } else {
              Some( (qual.to_term(), res) )
            }
          }

        }
      }
    }
    Ok(prev)
  }



  // /// Blacklists a qualifier.
  // pub fn blacklist(& mut self, qual: & Term) {
  //   let is_new = self.blacklist.insert( qual.clone() ) ;
  //   debug_assert! { is_new }
  // }
  // /// Clears the qualifier blacklist.
  // pub fn clear_blacklist(& mut self) {
  //   self.blacklist.clear()
  // }



  /// Inserts a new qualifier from a term.
  ///
  /// When a qualifier is inserted, it is considered new until it is evaluated
  /// for the first time.
  pub fn insert(
    & mut self, term: & Term, pred: PrdIdx
  ) -> bool {
    let pred_sig = & self.instance[pred].sig ;
    // This function basically renames the variables that appear in `term` so
    // that they are numbered in the order they appear in. While doing so, it
    // builds the following.

    // Signature of the alpha-renamed term.
    let mut sig = VarMap::with_capacity( pred_sig.len() ) ;
    // Map from variables of the alpha-renamed term to variables of `pred_sig`.
    let mut transform = VarMap::with_capacity( pred_sig.len() ) ;
    // Map from variables in `pred_sig` to variables of the alpha-renamed term.
    let mut map = VarHMap::with_capacity( pred_sig.len() ) ;

    // Stack used when going down applications. Elements of the stack are
    // triplets composed of
    //
    // - `op`: the operator of the application,
    // - `lft`: list of kids that have aready been handled,
    // - `rgt`: iterator over the kids, contains only those that haven't been
    //   handled yet.
    let mut stack: Vec<( Op, Vec<Term>, _ )> = Vec::with_capacity(7) ;
    // Term we're going down into.
    let mut curr = term ;

    let term = 'top_loop: loop {

      // Go down.
      let mut to_propagate_upward = match * * curr {

        // Variable, rename if necessary and move on.
        RTerm::Var(old_idx) => {
          use std::collections::hash_map::Entry ;
          let idx = match map.entry(old_idx) {
            Entry::Occupied(entry) => * entry.get(),
            Entry::Vacant(entry) => {
              let idx = sig.next_index() ;
              entry.insert(idx) ;
              sig.push( pred_sig[old_idx] ) ;
              transform.push(old_idx) ;
              idx
            },
          } ;
          term::var(idx)
        },

        // Constants, nothing to do.
        RTerm::Int(_) => curr.clone(),
        RTerm::Bool(_) => curr.clone(),

        // Application, we're going down.
        //
        // Push on stack, go down the first argument (update `curr`).
        RTerm::App { op, ref args } => {
          let mut args_iter = args.iter() ;
          if let Some(next) = args_iter.next() {
            curr = next ;
          } else {
            panic!("empty operator application")
          }
          stack.push( (op, Vec::with_capacity( args.len() ), args_iter) ) ;
          continue 'top_loop
        },

      } ;

      // Go up, working on `to_propagate_upward`.
      'go_up: loop {

        if let Some( (op, mut lft, mut rgt) ) = stack.pop() {
          // There something on the stack, update `lft`.
          lft.push(to_propagate_upward) ;
          if let Some(next) = rgt.next() {
            // There still terms to go down into, push back the frame on the
            // stack and go down `next`.
            stack.push( (op, lft, rgt) ) ;
            curr = next ;
            continue 'top_loop
          } else {
            // Nothing left for this application, go up the modified
            // application.
            to_propagate_upward = term::app(op, lft) ;
          }
        } else {
          // Nothing on the stack, we're done.
          break 'top_loop to_propagate_upward
        }

      }
    } ;

    if sig.is_empty() {
      // No variables, don't care about this term.
      return false
    }
    sig.shrink_to_fit() ;

    // Remove term's negation if any.
    let term = if let Some(term) = term.rm_neg() {
      term
    } else {
      term
    } ;

    // Hashcons signature.
    let sig = self.factory.mk(sig) ;

    // Insert in the classes.
    use std::collections::hash_map::Entry ;
    match self.classes.entry(sig) {
      Entry::Occupied(entry) => entry.into_mut().insert(
        term, pred, pred_sig, transform
      ),
      Entry::Vacant(entry) => {
        let transforms = SigTransforms::new(
          self.instance.preds(), entry.key()
        ) ;

        if let Some(class) = QualClass::new(transforms, 107) {
          entry.insert(class).insert(
            term, pred, pred_sig, transform
          )
        } else {
          false
        }
      },
    }
  }

  /// Prints itself.
  pub fn print(& self, pref: & str) {
    println!("{}quals {{", pref) ;
    for (sig, class) in & self.classes {
      let mut s = String::new() ;
      for (var, typ) in sig.index_iter() {
        s.push_str(& format!(" ({} {})", var.default_str(), typ))
      }
      println!("{}  sig{} {{", pref, s) ;
      s.clear() ;
      println!("{}    transforms {{", pref) ;
      for (sig, transs) in class.transforms.iter() {
        for (var, typ) in sig.index_iter() {
          s.push_str( & format!(" ({} {})", var.default_str(), typ) ) ;
        }
        println!("{}      for {}", pref, s) ;
        s.clear() ;
        for trans in transs.iter() {
          for (var, v) in trans.index_iter() {
            s.push_str(
              & format!(" {} -> {},", v.default_str(), var.default_str())
            )
          }
          println!("{}      |{}", pref, s) ;
          s.clear()
        }
      }
      println!("{}    }}", pref) ;
      for (quals, cache) in class.quals.iter() {
        println!("{}    {} ({})", pref, quals, cache.is_new) ;
        print!(  "{}    ->", pref) ;
        for pred in & cache.preds {
          print!(" {},", self.instance[* pred])
        }
        println!("")
      }
      println!("{}  }}", pref)
    }
    println!("{}}}", pref)
  }

}




/// Signature trait, for polymorphic term insertion.
pub trait Signature {
  /// Type of a variable.
  fn get(& self, VarIdx) -> Typ ;
  /// Length of the signature.
  fn len(& self) -> usize ;
}
impl Signature for VarMap<VarInfo> {
  fn len(& self) -> usize { VarMap::len(self) }
  fn get(& self, var: VarIdx) -> Typ {
    self[var].typ
  }
}
impl Signature for VarMap<Typ> {
  fn len(& self) -> usize { VarMap::len(self) }
  fn get(& self, var: VarIdx) -> Typ {
    self[var]
  }
}
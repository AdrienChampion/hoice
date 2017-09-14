//! Values used in evaluation.

use errors::* ;
use common::{ Int, Signed } ;


/// Values.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Val {
  /// Boolean value.
  B(bool),
  /// Integer value.
  I(Int),
  /// No value (context was incomplete).
  N,
}
impl Val {
  /// Extracts a boolean value.
  pub fn to_bool(self) -> Res<Option<bool>> {
    match self {
      Val::B(b) => Ok( Some(b) ),
      Val::I(_) => bail!("expected boolean value, found integer"),
      Val::N => Ok(None),
    }
  }
  /// Extracts an integer value.
  pub fn to_int(self) -> Res<Option<Int>> {
    match self {
      Val::B(_) => bail!("expected integer value, found boolean"),
      Val::I(i) => Ok( Some(i) ),
      Val::N => Ok(None),
    }
  }
  /// Value parser.
  #[allow(unused_variables)]
  pub fn parse(
    bytes: & [u8]
  ) -> ::nom::IResult<& [u8], Self, Error> {
    use common::parse::* ;
    fix_error!(
      bytes,
      Error,
      alt_complete!(
        map!( tag!("true"), |_| Val::B(true) ) |
        map!( tag!("false"), |_| Val::B(false) ) |
        map!( int, |i| Val::I(i) ) |
        do_parse!(
          char!('(') >>
          spc_cmt >> char!('-') >>
          spc_cmt >> value: int >>
          spc_cmt >> char!(')') >>
          ( Val::I(- value) )
        )
      )
    )
  }
}
impl_fmt!{
  Val(self, fmt) {
    match * self {
      Val::I(ref i) => if i.is_negative() {
        write!(fmt, "(- {})", - i)
      } else {
        write!(fmt, "{}", i)
      },
      Val::B(b) => write!(fmt, "{}", b),
      Val::N => fmt.write_str("?"),
    }
  }
}
impl From<bool> for Val {
  fn from(b: bool) -> Val {
    Val::B(b)
  }
}
impl From<Int> for Val {
  fn from(i: Int) -> Val {
    Val::I( i.into() )
  }
}
impl From<usize> for Val {
  fn from(i: usize) -> Val {
    Val::I( i.into() )
  }
}
impl From<isize> for Val {
  fn from(i: isize) -> Val {
    Val::I( i.into() )
  }
}
impl From<u32> for Val {
  fn from(i: u32) -> Val {
    Val::I( i.into() )
  }
}
impl From<i32> for Val {
  fn from(i: i32) -> Val {
    Val::I( i.into() )
  }
}
impl From<u64> for Val {
  fn from(i: u64) -> Val {
    Val::I( i.into() )
  }
}
impl From<i64> for Val {
  fn from(i: i64) -> Val {
    Val::I( i.into() )
  }
}
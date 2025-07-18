/*
 * Copyright © 2024, United States Government, as represented by the Administrator of 
 * the National Aeronautics and Space Administration. All rights reserved.
 *
 * The “ODIN” software is licensed under the Apache License, Version 2.0 (the "License"); 
 * you may not use this file except in compliance with the License. You may obtain a copy 
 * of the License at http://www.apache.org/licenses/LICENSE-2.0.
 *
 * Unless required by applicable law or agreed to in writing, software distributed under
 * the License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND,
 * either express or implied. See the License for the specific language governing permissions
 * and limitations under the License.
 */

#[allow(unused_macros)]

/// macro to flatten deeply nested "if let .." trees into a construct akin to Scala for-comprehensions
/// (or Haskell do-notation) with the extension that we can (optionally) specify side effects and/or
/// return values for failed matches.
/// 
/// Constraints:
///   - if there is an `else` clause both the match expression and the else clause have to be blocks
///     (this is a declarative macro constraint)
///   - to keep the syntax consistent we always require a ',' separator between non-terminal `if_let`` arms,
///     even if they end in blocks
///   - we use closures for `else` clauses that need the failed match expression value - note this means
///     the closure argument is a `Result` not an `Error`
///
/// This is an example for pure side effects:
/// ```
/// use odin_common::if_let;
/// let p: Option<i64> = ...
/// let q: Result<i64,&'static str> = ...
/// fn foo (n:i64, m:i64)->Result<&'static str,&'static str> { ... }
/// 
/// if_let! {
///     Some(a) = p,
///     Ok(b) = q,
///     Ok(c) = foo(a,b) => {
///         println!("just here to print a={}, b={}, c={}", a,b,c);
///     }
/// }
/// ```
/// which gets expanded into:
/// ```
/// if let Some(a) = p {
///     if let Ok(b) = q {
///         if let Ok(c) = foo(a, b) {
///             println!("just here to print a={}, b={}, c={}", a,b,c)
///         }
///     }
/// }
/// ```
/// 
/// Here is an example for pure side effects that uses fail closures (e.g. for error reporting)
/// ```
/// if_let! {
///     Some(a) = p,
///     Ok(b) = { q } else |other| { println!("no b: {other:?}") },
///     Ok(c) = { foo(a,b) } else |other| { println!("no c: {other:?}") } => {
///         println!("just here to print a={}, b={}, c={}", a,b,c);
///     }
/// }
/// ```
/// which expands to:
/// ```
/// if let Some(a) = p {
///     match { q } {
///         Ok(b) => {
///             match { foo(a, b) } {
///                 Ok(c) => { println!("just here to print a={}, b={}, c={}", a,b,c) }
///                 x => { |e|{ println!("no c: {e:?}") }(x) }
///             }
///         }
///         x => { |e|{println!("no b: {e:?}")}(x) }
///     }
/// } 
/// ```
/// This is finally an example that uses the `if_let` value and fail closures (providing failure values)
/// ```
/// let res = if_let! {
///     Some(a) = { p } else { println!("no a"); -1 },
///     Ok(b)   = { q } else |e| { println!("no b: {e:?}"); -2 }, 
///     Ok(c)   = { foo(a,b) } else |e| { println!("no c: {e:?}"); -3 } => {
///         println!("a={}, b={}, c={}", a,b,c);
///         0
///     }
/// };
/// println!("res = {res}");
/// ``` 
/// which is expanded into:
/// ```
/// let res = if let Some(a) = { p } {
///     match { q } {
///         Ok(b) => {
///             match { foo(a, b) } {
///                 Ok(c) => { println!("a={}, b={}, c={}", a,b,c); 0 }
///                 x => { |e|{ println!("no c: {e:?}"); -3 }(x) }
///             }
///         }
///         x => { |e|{ println!("no b: {e:?}"); -2 }(x) }
///     }
/// } else { println!("no a"); -1 };
/// println!("res = {res}");
/// ```
#[macro_export]
macro_rules! if_let {
    //--- the leafs
    { $p:pat = $x:block else $e:block => $r:expr } => {
        if let $p = $x { $r } else $e
    };
    { $p:pat = $x:block else $closure:expr => $r:expr } => {
        match $x {
            $p => { $r }
            other => { $closure( other) }
        }
    };
    { $p:pat = $x:expr => $r:expr } => {
        if let $p = $x { $r } 
    };
    
    //--- the recursive tt munchers
    { $p:pat = $x:block else $e:block , $($ts:tt)+ } => {
        if let $p = $x { if_let! { $($ts)+ } } else $e
    };
    { $p:pat = $x:block else $closure:expr , $($ts:tt)+ } => { // expr covers closures
        match $x {
            $p => { if_let! { $($ts)+ } }
            other => { $closure( other) } // watch out - 'other' type is not Error but Result
        }
    };
    { $p:pat = $x:expr , $($ts:tt)+ } => {
        if let $p = $x {
            if_let! { $($ts)+ }
        }
    };
}
pub use if_let; // preserve 'macros' module across crates

/// syntactic sugar for "format!(...).as_str()" - can only be used for arguments, not to bind variables
#[macro_export]
macro_rules! str {
    ( $fmt:literal, $($arg:expr),* ) =>
    {
        format!($fmt,$($arg),*).as_str()
    }
}
pub use str;

#[macro_export]
macro_rules! push_all_str {
    ( $o:ident, $($arg:expr),+ ) =>
    {
        $( $o.push_str( $arg) );+
    }
}
pub use push_all_str;


#[macro_export]
macro_rules! max {
    ($x:expr) => ( $x );
    ($x:expr, $($xs:expr),+) => {
        $x.max( max!( $($xs),+))
    };
}
pub use max;

#[macro_export]
macro_rules! min {
    ($x:expr) => ( $x );
    ($x:expr, $($xs:expr),+) => {
        $x.min( min!( $($xs),+))
    };
}
pub use min;


/// macros to reduce boilerplate for coercing errors into custom lib errors.
/// This is a simplistic alternative to the 'thiserror' crate. It is aimed at providing
/// specific, matchable error types for lib crates, optionally preserving underlying error chains
/// and keeping code clutter minimal.
///
/// The downside is that in it's current form it only provides a fixed, non-extensible error structure
///
/// single type use case is like so:
/// ```
/// define_err!(BarError);
///
/// fn bar (mut s: String) -> Result<(),BarError> {
///     map_err!( s.try_reserve(42) => BarError{"could not extend string capacity"})?;
///     Ok(())
/// }
///
/// fn main () {
///    match bar("somewhat".to_string()) {
//         Ok(()) => println!("bar Ok"),
//         Err(e) => println!("bar error = {}", e)
//     }
/// }
/// ```
///
/// error enums can be defined/used like so:
/// ```
/// define_err!(FooError: FooInputError,FooOutputError);
///
/// fn foo (bs: &[u8]) -> Result<String,FooError> {
///     use FooError::*;
///     // let cs = CString::new(bs).map_err(|e| FooInputError{source:Box(e),msg: format!("0-byte in input: {:?}",bs))?;
///     // let s = cs.into_string().map_err(|e| FooInputError{source:Box(e),msg: format!("malformet utf8")?;
///
///     let cs = map_err!( CString::new(bs) => FooInputError{"0-byte in input: {:?}", bs})?;
///     let s = map_err!( cs.into_string() => FooInputError{"malformed utf8"})?;
///     return_err!( s.len() > 8 => FooOutputError{"output too long: {}", bs.len()});
///
///     Ok(s)
/// }
///
/// fn main () {
///     use FooError::*;
///     let bs: &[u8] = &[104, 101, 0, 108, 108, 111];
///
///     match foo(bs) {
///        Ok(s) => println!("Ok(s) = {}", s),
///        Err(ref e) => match e {
///          FooInputError{src,msg} => println!("foo input error = {}", e),
///          FooOutputError{src,msg} => println!("foo output error = {}", e)
///        }
///     }
/// ```


#[macro_export]
macro_rules! define_err {
    ($t:ident) =>
    { #[derive(Debug)]
      pub struct $t { src: Option<Box<dyn Error + Send + Sync + 'static>>, msg: Option<String> }
      impl Error for $t {
          fn source(&self) -> Option<&(dyn Error + 'static)> {
             match &self.src {
                 Some(b) => Some(b.as_ref()),
                 None => None
             }
          }
      }
      impl fmt::Display for $t {
          fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            match self.msg {
                Some(ref msg) => write!( f, "{}: {}", stringify!($t), msg),
                None => write!( f, "{}", stringify!($t))
            }
          }
      }
    };

    ($t:ident : $($m:ident),+) =>
    {   #[derive(Debug)]
        pub enum $t {
            $($m { src: Option<Box<dyn Error + Send + Sync + 'static>>, msg: Option<String> },)+
        }
        impl Error for $t {
            fn source(&self) -> Option<&(dyn Error + 'static)> {
                use $t::*;
                match *self {
                    $(
                      $m{src:ref src,msg:_} => match src {
                          Some(b) => Some(b.as_ref()),
                          None => None
                      }
                    )+
                }
            }
        }
        impl fmt::Display for $t {
          fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            use $t::*;
            match *self {
                $(
                  $m{src:_,msg:ref msg} => match msg {
                    Some(ref msg) => write!( f, "{}::(): {}", stringify!($t),stringify!($m), msg),
                    None => write!( f, "{}::{}", stringify!($t),stringify!($t)),
                  }
                )+
            }
          }
        }
    };
}
pub use define_err;

#[macro_export]
macro_rules! return_err {
    ($e:expr => $t:ident ) =>
    { if $e { return Err($t{ src:None, msg:None}) } };

    ($e:expr => $t:ident { $f:literal $(, $p:expr)* } ) =>
    { if $e { return Err($t{ src:None, msg:Some(format!($f $(,$p)* ))}) } };
}
pub use return_err;

#[macro_export]
macro_rules! map_err {
    ($e:expr => $t:ident ) =>
    { $e.map_err(|err| $t{ src:Some(Box::new(err)), msg:None}) };

    ($e:expr => $t:ident { $f:literal $(, $p:expr)* } ) =>
    { $e.map_err(|err| $t{ src:Some(Box::new(err)), msg:Some(format!($f $(,$p)* ))}) };
}
pub use map_err;

#[macro_export]
macro_rules! io_error {
    ( $kind:expr, $fmt:literal $(, $($arg:expr),* )? ) =>
    {
        io::Error::new( $kind, format!($fmt, $( $($arg),* )?).as_str())
    }
}
pub use io_error;

//--- macros that are usefule to map errors (also to thiserror defined enums)

#[macro_export]
macro_rules! map_to_opaque_error {
    ($from_error:ty => $to_error:ident :: $variant:ident) => {
        impl From<$from_error> for $to_error {
            fn from (e: $from_error)->Self { $to_error :: $variant ( e.to_string()) }
        }
    };
}

//--- feature management (conditional compilation support)

#[macro_export]
macro_rules! assert_unique_feature {
    () => {};
    ($first:tt $(,$rest:tt)*) => {
        $(
            #[cfg(all(feature = $first, feature = $rest))]
            compile_error!(concat!("features \"", $first, "\" and \"", $rest, "\" are mutually exclusive"));
        )*
        assert_unique_feature!($($rest),*);
    }
}

/* #region define_cli  ****************************************************************************************/

/// syntactic sugar macro for structopt based command line interface definition
/// ```
/// define_cli! { ARGS [about="my silly prog"] = 
///   verbose: bool        [help="run verbose", short],
///   date: DateTime<Utc>  [help="start date", from_os_str=parse_utc_datetime_from_os_str_date],
///   config: String       [help="pathname of config", long, default_value="blah"]
/// }
/// 
/// fn main () {
///    check_cli!(ARGS); // makes sure we exit on -h or --help (and do not execute anything until we know ARGS parsed)
///    ... 
///    let config = &ARGS.config; 
///    ...
/// }
/// ```
/// expands into:
/// ```
/// use structopt::StructOpt;
/// use lazy_static::lazy_static;
/// 
/// #[derive(StructOpt)]
/// #[structopt(about = "my silly prog")]
/// struct CliOpts {
///     #[structopt(help = "run verbose", short)]
///     verbose: bool,
/// 
///     #[structopt(help = "blah date", from_os_str=parse_utc_datetime_from_os_str_date)]
///     date: DateTime<Utc>,
/// 
///     #[structopt(help = "pathname of config", long, default_value = "blah")]
///     config: String,
/// 
///     #[structopt(skip=true)] // hidden field to check initialization without referencing any of the arg fields
///     _initialized: bool
/// }
/// 
/// fn main () {
///    { let _is_initialized = &ARGS._initialized; }
///    ...
/// }
/// ```
#[macro_export]
macro_rules! define_cli {
    ($name:ident [ $( $sopt:ident $(= $sx:expr)? ),* ] = $( $( #[$meta:meta] )? $fname:ident : $ftype:ty [ $( $fopt:ident $(= $fx:expr)?),* ] ),* ) => {
        use structopt::StructOpt;
        use lazy_static::lazy_static;

        #[derive(StructOpt)]
        #[structopt( $( $sopt $(=$sx)? ),* )]
        struct CliOpts {
            $(
                #[structopt( $( $fopt $(=$fx)? ),* )]
                $(#[$meta])?
                $fname : $ftype,
            )*
            #[structopt(skip=true)]
            _initialized: bool
        }
        lazy_static! { static ref $name: CliOpts = CliOpts::from_args(); }
    }
}

#[macro_export]
macro_rules! check_cli {
    ($sopt:ident) => { { let _is_initialized = &$sopt._initialized; } }
}

/// syntactic sugar macro to define thiserror Error enums:
/// ```
/// define_error!{ pub OdinNetError = 
///   IOError( #[from] std::io::Error ) : "IO error: {0}",
///   OpFailed(String) : "operation failed: {0}"
/// }
/// ```
/// will get expanded into
/// ```
/// use thiserror;
/// pub enum OdinNetError {
///     #[error("IO error: {0}")]
///     IOError(#[from] std::io::Error),
/// 
///     #[error("operation failed: {0}")]
///     OpFailed(String),
/// }
/// ```
#[macro_export]
macro_rules! define_error {
    ($vis:vis $name:ident = $( $err_variant:ident ( $( $( #[$meta:meta] )? $field_type:ty),* ) : $msg_lit:literal ),*) => {
        use thiserror;
        #[derive(thiserror::Error,Debug)]
        $vis enum $name {
            $( 
                #[error($msg_lit)]
                $err_variant ( $( $(#[$meta])? $field_type ),*  )
            ),*
        }
    }
}

/* #endregion define_cli */


/// syntactic sugar macro to expand into a struct with serde attribute macros
/// This mostly expands optional "[ attr,.. ]" groups into respective #[serde(attrs...)] container or field attribute macros
/// use like this:
/// 
/// define_serde_struct! {
///     pub GetMapQuery : Debug [deny_unknown_fields] = 
///        service: String [alias = "svc", default="default_service"],
///        layers: Option<String>
/// }
/// 
/// TODO - we might turn this into q proc macro so that we can also do ad hoc default value spec without the need for additional functions
#[macro_export]
macro_rules! define_serde_struct {
    ( $vis:vis $name:ident $( : $( $dt:ty),* )? $( [ $( $sopt:ident $(= $sx:literal)? ),* ] )? = 
       $( $( #[$fmeta:meta] )? $fvis:vis $fname:ident: $ftype:ty $( [ $( $fopt:ident $(= $fx:literal)? ),* ] )? ),*  $(,)?) => {
        #[derive(Serialize,Deserialize $( $( , $dt)* )? )]
        $( #[serde( $( $sopt $( = $sx)? ),* ) ])?
        $vis struct $name {
            $( 
                $( #[ $fmeta ] )?
                $( #[serde(  $( $fopt $( =$fx )?),*  )] )?
                $fvis $fname : $ftype
            ),*
        }
    }
}

/* #region impl_virt_deserialize *********************************************************************************/

/// implement serde Deserialize trait not backed by structure fields
/// This macro uses arguments to define structure name, a constructor fn name and virtual deserialization field names to 
/// generate a Deserializer impl. It also suppors optional aliases for each field name.
/// The constructor has to accept the deserialized values in the order specified. The constructor argument types define
/// the respective argument deserialization
/// 
/// Use like so:
/// ```
/// struct GeoPoint {...}
/// impl GeoPoint {
///   fn from_lon_lat_degrees (lon: .., lat: ..)->Self {...}
/// }
/// 
/// impl_deserialize_struct!{ GeoPoint::from_lon_lat_degrees( lon, ["longitude", "x"], lat, ["latitude", "y"]) }
/// ```
#[macro_export]
macro_rules! impl_deserialize_struct {
    ( $type_name:ident :: $ctor_name:ident ( $( $field_name:ident $( | $alt_name:ident )*  $( = $def_val:expr )? ),+ )) => {
        impl<'de> DeserializeTrait<'de> for $type_name {
            fn deserialize<D>(deserializer: D) -> Result<$type_name, D::Error> where D: Deserializer<'de> {
                const FIELDS: &[&str] = &[ $( stringify!($field_name) ),+ ];

                #[allow(non_camel_case_types)]
                enum Field { $( $field_name ),+ }

                impl<'de> DeserializeTrait<'de> for Field {
                    fn deserialize<D>(deserializer: D) -> Result<Field, D::Error> where D: Deserializer<'de> {
                        struct FieldVisitor;

                        impl<'de> Visitor<'de> for FieldVisitor {
                            type Value = Field;

                            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                                formatter.write_str( stringify!( $($field_name),+) )
                            }

                            fn visit_str<E>(self, value: &str) -> Result<Field, E> where E: de::Error {
                                match value {
                                    $( stringify!($field_name) $( | stringify!($alt_name) )* => Ok(Field::$field_name), )+
                                    _ => Err(de::Error::unknown_field(value, FIELDS)),
                                }
                            }
                        }
                        deserializer.deserialize_identifier(FieldVisitor)
                    }
                }

                struct TypeVisitor;

                impl<'de> Visitor<'de> for TypeVisitor {
                    type Value = $type_name;

                    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                        formatter.write_str( stringify!( struct $type_name) )
                    }

                    fn visit_seq<V>(self, mut seq: V) -> Result< $type_name, V::Error> where V: SeqAccess<'de> {
                        $( let $field_name = seq.next_element()?.ok_or_else(|| de::Error::invalid_length( Field::$field_name as usize, &self))?; )+
                        Ok( $type_name::$ctor_name( $( $field_name ),+ ) )
                    }

                    fn visit_map<V>(self, mut map: V) -> Result< $type_name, V::Error> where V: MapAccess<'de> {
                        $( let mut $field_name = None; )+

                        while let Some(key) = map.next_key()? {
                            match key {
                                $(
                                    Field::$field_name => {
                                        if $field_name .is_some() { {return Err(de::Error::duplicate_field( stringify!($field_name)));} }
                                        $field_name = Some(map.next_value()?);
                                    }
                                )+
                            }
                        }

                        $( 
                            $( let $field_name = $field_name.or( Some($def_val) ); )?
                            let $field_name = $field_name .ok_or_else(|| de::Error::missing_field(stringify!($field_name)))?; 
                        )+
                        Ok( $type_name::$ctor_name( $( $field_name ),+ ) )
                    }
                }

                deserializer.deserialize_struct( stringify!($type_name), FIELDS, TypeVisitor)
            }
        }
    }
}

/// implement serde Deserialize trait for a type from a given constructor name that takes a single Vec<ElementType> argument.
/// The element type has to have a Deserialize impl
/// 
/// Use like so:
/// ```
/// struct GeoPoint {...} 
/// impl <'de> Deserialize for GeoPoint { ... }
/// 
/// struct GeoLineString {...}
/// impl GeoLineString {
///     fn from_geo_points (points: Vec<GeoPoint>)->Self {...}
/// }
/// 
/// impl_deserialize_seq!{  GeoLineString::from_geo_points(Vec<GeoPoint>) }
/// ```
#[macro_export]
macro_rules! impl_deserialize_seq {
    ( $type_name:ident :: $ctor_name:ident ( Vec < $elem_type:ident > ) ) => {
        impl<'de> DeserializeTrait<'de> for $type_name {
            fn deserialize<D>(deserializer: D) -> Result<$type_name, D::Error> where D: Deserializer<'de> {
                struct TypeVisitor;
        
                impl<'de> Visitor<'de> for TypeVisitor {
                    type Value = GeoLineString;
        
                    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                        formatter.write_str( stringify!($type_name as seq of $elem_type elements))
                    }
        
                    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error> where A: SeqAccess<'de> {
                        let mut elems: Vec<$elem_type> = Vec::new();
        
                        while let Some(p) = seq.next_element::<$elem_type>()? {
                            elems.push( p);
                        }
        
                        Ok( $type_name::$ctor_name( elems) )
                    }
                }
        
                deserializer.deserialize_seq( TypeVisitor)
            }
        }
    }
}

/* #endregion impl_deserialize_struct */

#[macro_export]
macro_rules! arc {
    ($s:literal) => {
        Arc::new( $s.to_string() )
    };
    ($s:expr) => {
        Arc::new( $s.to_string() )
    }
}
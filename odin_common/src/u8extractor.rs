/*
 * Copyright © 2025, United States Government, as represented by the Administrator of 
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

use memchr::memmem::Finder;

/// this module provides support to extract keyed values from binary data

/*
  ......xxxxxxxDDDDDDD........... msg
        i      j     k
        |      |--d--| (var)
        |--L---|     data-end 
        |      data-start (i + L)
        key-start

    //--- with guaranteed order 
    if let Some(i1) = find1.find_key(msg)
    && let Some((v1,d1)) = read_val::<T>( msg, i1+len(find1))
    && let Some(i2) = find2.find_key(msg[i1+d1..])
    && let Some(v2,d1)) = read_val::<T>( msg, i1+d1+i2)
    ...

    //--- without guaranteed order
    if let Some(i1) = find1.find(msg)
    && let Some(v1,_) = read_val::<T>( msg, i1+len(find1))
    && let Some(i2) = find2.find(msg)
    && let Some(v2,_) = read_val::<T>( msg, i2+len(find2))
    ...
*/


/// this macro is the main export of this module - it is syntactic suger for if-let chains that
/// find keys and respective values in a given &[u8] buffer. 
/// Use like so:
/// ```
///     use odin_common::u8extractor::{extract,SimpleU8Finder};
/// 
///     let buf: &[u8] = b"{\"key1\":42,\"key2\":\"foo\"}";
///    
///     let find_key1 = SimpleU8Finder::new(b"\"key1\":");
///     let find_key2 = SimpleU8Finder::new(b"\"key2\":\"");
///    
///     println!("haystack={}", str::from_utf8(buf).unwrap());
///    
///     extract! { buf ? 
///         let v1: u64 = find_key1,
///         let v2: &str = find_key2 => {
///             println!("got v1={v1}, v2={v2}");
///         }
///     }
///```
/// 
/// which gets expanded into:
/// ```
///     ...
///     if let Some(i) = extract_key1.find_key(buf)
///     && let Some((v1,len)) = U8Readable::from_u8::<u64>( buf, i + extract_key1.len())
///     && let Some(i) = extract_key12.find_key(buf)
///     && let Some((v2,len)) = U8Readable::from_u8::<&str>( buf, i + extract_key2.len()) {
///         println!("got v1={v1}, v2={v2}");
///     }
/// ```
/// 
#[macro_export]
macro_rules! extract_all {
    ($buf:ident ? $( let $var:ident : $vt:ty = $f:expr ),* => $blk:block $( else $else_blk:block )?) => {
      {
         if
         $(
           let Some(i) = $f.find_key($buf)
           && let Some(($var,_)) = odin_common::u8extractor::read_val::<$vt>( $buf, i + $f.len()) 
         )&&*
         $blk
         $( else $else_blk )?
      }
    }
}

#[macro_export]
macro_rules! extract_ordered {
    ($buf:ident ? $( let $var:ident : $vt:ty = $f:expr ),* => $blk:block $( else $else_blk:block )?) => {
      {
         let mut i0 = 0;
         if
         $(
           let Some(i) = {
             $f.find_key( &$buf[i0..])
           }
           && let Some(($var,_)) = { 
             let j = i0 + i + $f.len();
             let res = odin_common::u8extractor::read_val::<$vt>( $buf, j);
             if res.is_some() { 
               i0 = j + res.unwrap().1 
             }
             res
           } 
         )&&*
         $blk
         $( else $else_blk )?
      }
    }
}

#[macro_export]
macro_rules! extract_optional {
    ($buf:ident, $f:expr, $vt:ty) => {
        if let Some(idx) = $f.find_key( $buf) { 
            <$vt>::from_u8( $buf, idx + $f.len()).map( |(v,_)| v)
        } else { None };
    }
}

pub fn read_val<'a,T> (haystack: &'a [u8], i0: usize)-> Option<(T,usize)> where T: U8Readable<'a,T> {
    T::from_u8( haystack, i0)
}

/// this is a simplistic extractor that just forward iterates over the haystack to find the needle
/// use if haystacks/needles are short. There is no instantiation cost i.e. SimpleFinder instances can
/// be created on-the-fly
pub struct SimpleU8Finder { 
    pub needle: &'static [u8] 
}

impl SimpleU8Finder {
    pub fn new (needle: &'static [u8])->Self { 
        SimpleU8Finder { needle } 
    }
    pub fn len(&self)->usize { self.needle.len() }
    
    pub fn find_key (&self, haystack: &[u8])->Option<usize> {
        let mut j:usize = 0;
        let mut i:usize = 0;
        let mut i0:usize = 0;
    
        loop {
            if haystack[i] == self.needle[j] {
                if j == 0 {
                    i0 = i
                }
                j += 1;
                if j >= self.needle.len() {
                    return Some(i0);
                } else {
                    i += 1;
                    if i >= haystack.len() {
                        return None;
                    }
                }
            } else {
                if j > 0 {
                    j = 0;
                } else {
                    i += 1;
                    if i >= haystack.len() {
                        return None;
                    }
                }
            }
        }
    }
}

/// is a more complex finder that wraps a memchr::memmem::Finder which uses SIMD instructions
/// to speed up the search for longer hackstacks and needles.
/// This incurs instantiation cost and hence should be done upfront 
pub struct MemMemFinder<'a> (Finder<'a>);

impl<'a> MemMemFinder<'a> {
    #[inline]
    pub fn new (needle: &'static [u8])->Self {  MemMemFinder(Finder::new(needle)) }

    #[inline]
    pub fn len(&self)->usize { self.0.needle().len() }
    
    #[inline]
    pub fn find_key (&self, haystack: &[u8])->Option<usize> { self.0.find( haystack) }
}

/// a type that can be read from an &[u8] buffer and lives at least as long as this buffer
pub trait U8Readable<'a,T> {
    /// return tuple with value and u8 length of value if buf[i] marks the beginning of a valid representation, None otherwise
    fn from_u8 (buf: &'a[u8], i: usize)->Option<(T,usize)>;
}

/* #region U8Readable implementations **************************************************************************/

// we only have stanard type impls here - clients can provide their own specialized U8Readable implementations 
// for the types they want to extract

impl<'a> U8Readable<'a,u64> for u64 {
    fn from_u8 (buf: &'a[u8], i0: usize)->Option<(u64,usize)> {
        let mut i = i0;
        let mut n: u64 = 0;
        loop {
            if i >= buf.len() { return None }

            let b: u8 = buf[i];
            if b >= b'0' && b <= b'9' {
                n = n * 10 + (b as u64 - 48);
            } else {
                if i == i0 {
                    return None;
                } else {
                    return Some((n, i-i0));
                }
            }
            i += 1;
        }
    }
}

impl<'a> U8Readable<'a,i64> for i64 {
    fn from_u8 (buf: &'a[u8], i0: usize)->Option<(i64,usize)> {
        let mut i = i0;
        let mut n: i64 = 0;
        let mut sig: i64 = 1;

        if buf[i] == b'-' {
            sig = -1;
            i += 1;
        }

        loop {
            if i >= buf.len() { return None }

            let b: u8 = buf[i];
            if b >= b'0' && b <= b'9' {
                n = n * 10 + (b as i64 - 48);
            } else {
                if i == i0 {
                    return None;
                } else {
                    return Some((sig * n, i-i0));
                }
            }
            i += 1;
        }
    }
}

impl<'a> U8Readable<'a, &'a str> for &'a str {
    fn from_u8 (buf: &'a[u8], i0: usize)->Option<(&'a str,usize)> {
        let mut i = i0;
        let mut skip = false;
        loop {
            if i >= buf.len() { return None }
            let b: u8 = buf[i];

            if !skip {
                if b == b'\\' {
                    skip = true;
                    continue;
                }
            } else {
                skip = false;
                continue;
            }
    
            if b == b'"' {
                unsafe { return Some((str::from_utf8_unchecked(&buf[i0..i]), i-i0)) }
            }
            
            i += 1;
        }
    }
}

impl<'a> U8Readable<'a,f64> for f64 {
    fn from_u8 (buf: &'a[u8], i0: usize)->Option<(f64,usize)> {
        let mut n: i64 = 0;
        let mut d: i64 = 0;
        let mut a: &mut i64 = &mut n;
        let mut sig = 1.0;
        let mut di = 0;

        let mut i = i0;
        if buf[i] == b'-' {
            sig = -1.0;
            i += 1;
        }

        loop {
            if i >= buf.len() { return None }

            let b: u8 = buf[i];
            if b >= b'0' && b <= b'9' {
                *a = *a * 10 + (b as i64 - 48);
            } else if b == b'.' {
                a = &mut d;
                di = i;
            } else {
                let x = sig * ((n as f64) + (d as f64) / 10.0f64.powi((i - di - 1) as i32));
                return Some((x, di));
            }

            i += 1;
        }
    }
}

/* #endregion U8Readable impls */
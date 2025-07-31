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

use std::{io::{self, BufRead}, ops::Deref};
use tokio::io::AsyncBufReadExt;
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


/* #region CSV extractor support *******************************************************************************/


/// macro to extract CSV fields from an CsvExtractor
///  
/// ```
///     extraxt_fields{ line ?
///         let spd: f64 = [4],
///         let vrate: i64 = [7] => {
///            ...
///         }
///     }
/// ```
/// 
/// which is expanded to 
/// 
/// ```
///     if let Some(spd) = line.field::<f64>(4)
///     && let Some(vrate) = line.field::<i64>(7) {
///         ...
///     }
/// ```
#[macro_export]
macro_rules! extract_fields {
    ($csv:ident ? $( let $v:ident : $vt:ty = [$i:expr] ),*  => $blk:block $( else $else_blk:block )?) => {
        {
            if
            $(
                let Some($v) = $csv.field::<$vt>($i)
            )&&*
            $blk
            $( else $else_blk )?
        }
    };
}

const SEP: u8 = b',';

/// stream like object to extract fields from CSV lines read from an underlying BufRead impl
/// the main purpose of this construct is to get field boundaries for each line once (without allocation)
/// use like so:
/// 
/// ```
///    let data = b",\"foo, bar\",42,\r\none,two,43";
///    let cursor = std::io::Cursor::new(data);
/// 
///    let mut csv = CsvExtractor::new(cursor);
///
///    while csv.next_line() {
///        println!("---- {}", csv.line());
///        println!("[0] = {:?}", csv.field::<CsvStr>(0));
///        println!("[1] = {:?}", csv.field::<CsvStr>(1));
///        println!("[2] = {:?}", csv.field::<i64>(2));
///    }
/// ```
pub struct CsvExtractor<R> where R: BufRead {
    reader: R,
    line: String,
    sep_indices: Vec<usize>,
}

impl<R> CsvExtractor<R> where R: BufRead {
    pub fn new (reader: R)->Self {
        CsvExtractor {
            reader,
            line: String::with_capacity(1024),
            sep_indices: Vec::new()
        }
    }
    
    pub fn field<'a,T: U8Readable<'a,T>> (&'a self, field_index: usize)->Option<T> {
        get_field( self.line.as_bytes(), &self.sep_indices.as_ref(), field_index)
    }
    
    pub fn next_line(&mut self) -> Result<bool,io::Error> {
        self.line.clear();
        self.sep_indices.clear();
        match self.reader.read_line(&mut self.line) {
            Ok(len) => {
                if len > 0 {
                    if self.line.as_bytes()[len - 1] == b'\n' { self.line.pop(); }
                    if self.line.as_bytes()[len - 2] == b'\r' { self.line.pop(); } // windows
                    set_separator_indices(&mut self.sep_indices, SEP, self.line.as_bytes());
                    Ok(true)
                } else { Ok(false) } // mp more data
            }
            Err(e) => Err( io::Error::new( io::ErrorKind::Other, e))
        }
    }

    pub fn line(&self) -> &str { self.line.as_str() }
}

/// the async version of `CsvExtractor` - obtaining a new line has to be awaited
pub struct AsyncCsvExtractor<R> where R: AsyncBufReadExt + Unpin {
    reader: R,
    line: String,
    sep_indices: Vec<usize>,
}

impl<R> AsyncCsvExtractor<R> where R: AsyncBufReadExt + Unpin {
    pub fn new (reader: R)->Self {
        AsyncCsvExtractor {
            reader,
            line: String::with_capacity(1024),
            sep_indices: Vec::new()
        }
    }
    
    pub fn field<'a,T: U8Readable<'a,T>> (&'a self, field_index: usize)->Option<T> {
        get_field( self.line.as_bytes(), &self.sep_indices.as_ref(), field_index)
    }
    
    pub async fn next_line(&mut self) -> Result<bool,io::Error> {
        self.line.clear();
        self.sep_indices.clear();
        match self.reader.read_line(&mut self.line).await {
            Ok(len) => {
                if len > 0 {
                    if self.line.as_bytes()[len - 1] == b'\n' { self.line.pop(); }
                    if self.line.as_bytes()[len - 2] == b'\r' { self.line.pop(); } // windows
                    set_separator_indices(&mut self.sep_indices, SEP, self.line.as_bytes());
                    Ok(true)
                } else { Ok(false) } // mp more data
            }
            Err(e) => Err( io::Error::new( io::ErrorKind::Other, e))
        }
    }

    pub fn line(&self) -> &str { self.line.as_str() }
}

pub fn get_field<'a,T: U8Readable<'a,T>> (buf: &'a[u8], sep_indices: &[usize], field_index: usize)->Option<T> {
    if field_index > sep_indices.len() { return None }
    let i = if field_index == 0 { 0 } else { sep_indices[field_index-1] + 1 };
    if i >= buf.len() || buf[i] == SEP { return None } 
    read_val( buf, i).map( |(v,_)| v)
}

// skip over double-quoted strings
fn set_separator_indices (indices: &mut Vec<usize>, sep: u8, buf: &[u8]) {
    indices.clear();
    
    let len = buf.len();
    let mut i=0;
    let mut skip = false;
    while i < len {
        if !skip {
            if buf[i] == b'"' { skip = true; }
            else if buf[i] == sep { indices.push(i) }
        } else {
            if buf[i] == b'"' { skip = false; }
        }
        
        i += 1;
    }
}

/* #endregion CSV extractor */


/* #region U8Readable implementations **************************************************************************/

// we only have stanard type impls here - clients can provide their own specialized U8Readable implementations 
// for the types they want to extract

pub trait U8Readable<'a,T> {
    /// return tuple with value and u8 length of value if buf[i] marks the beginning of a valid representation, None otherwise
    fn from_u8 (buf: &'a[u8], i: usize)->Option<(T,usize)>;
}


impl<'a> U8Readable<'a,u64> for u64 {
    fn from_u8 (buf: &'a[u8], i0: usize)->Option<(u64,usize)> {
        let mut i = i0;
        let mut n: u64 = 0;
        loop {
            if i >= buf.len() {
                return if i>i0 { Some((n, i-i0)) } else { None }
            }

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
            if i >= buf.len() { 
                return if i>i0 { Some((n, i-i0)) } else { None }
            }

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
            if i > buf.len() { 
                return if i>i0 { 
                    let x = sig * ((n as f64) + (d as f64) / 10.0f64.powi((i - di - 1) as i32));
                    Some((x, di)) 
                } else { None } 
            }

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


/// a newtype to wrap `&str` instances from CSV sources (which do not have to be '"' limited)
#[derive(Debug)]
pub struct CsvStr<'a>(&'a str);

impl<'a> CsvStr<'a> {
    pub fn as_str(&'a self)->&'a str { self.0 } 
}

impl<'a> Deref for CsvStr<'a> {
    type Target = &'a str;
    
    fn deref(&self)->&Self::Target { &self.0 }
}

impl<'a> U8Readable<'a, CsvStr<'a>> for CsvStr<'a> {
    fn from_u8 (buf: &'a[u8], i0: usize)->Option<(CsvStr<'a>,usize)> {
        let mut i0 = i0;
        let mut skip = false;
        
        let sep = if buf[i0] == b'"' { i0+=1; b'"' } else { b',' };
        let mut i = i0;
        
        loop {
            if i >= buf.len() { 
                if i>i0 {
                    unsafe { 
                        let s = str::from_utf8_unchecked(&buf[i0..i]);
                        return Some( (CsvStr::<'a>(s), i-i0) ) 
                    }
                } else {
                    return None
                }
            }

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
    
            if b == sep || b == b'\n' || i == buf.len()-1 {
                unsafe { 
                    let s = str::from_utf8_unchecked(&buf[i0..i]);
                    return Some( (CsvStr::<'a>(s), i-i0) ) 
                }
            }
            
            i += 1;
        }
    }
}


/* #endregion U8Readable impls */
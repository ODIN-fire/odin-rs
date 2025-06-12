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

#![allow(unused)]

use std::fmt::{Write,Display,Debug};
use std::borrow::Borrow;

/// a simple standalone JSON writer that produces JSON strings from  (nested) closures.
/// Useful for conditional serialization that would overstress serde (e.g. because of
/// conditional field serialization or multiple/dynamic value sources). Use like so:
/// ```
///     let x = 42.0;
///     let y = 43;
///     
///     let mut w = JsonWriter::new();
///     w.write_object( |w|{
///         w.write_fmt_field( "foo", &format!("{:.3}", x));
///         w.write_field( "bar", y);
///         w.write_array_field( "baz", |w|{
///             w.write_string_value("boo");
///             w.write_string_value("faz");
///         }) 
///     });
///     
///     println!("{}", w.to_string());
/// ```
pub struct JsonWriter {
    buf: String
}

impl JsonWriter {
    pub fn new()->Self { 
        JsonWriter { buf: String::new() } 
    }

    pub fn with_capacity (len: usize)->Self { 
        JsonWriter { buf: String::with_capacity(len) } 
    }
    
    pub fn write_object (&mut self, f: impl FnOnce(&mut JsonWriter)) { 
        self.check_separator();
        self.buf.write_char('{');
        f (self);
        self.buf.write_char('}');
    }
    
    pub fn write_object_field (&mut self, prop_name: &str, f: impl FnOnce(&mut JsonWriter)) { 
        self.check_separator();
        write!( self.buf, "\"{prop_name}\":");
        self.buf.write_char('{');
        f (self);
        self.buf.write_char('}');
    }

    pub fn write_array (&mut self, f: impl FnOnce(&mut JsonWriter)) {
        self.check_separator();
        self.buf.write_char('[');
        f (self);
        self.buf.write_char(']');
    }
    
    pub fn write_array_field (&mut self, prop_name: &str, f: impl FnOnce(&mut JsonWriter)) { 
        self.check_separator();
        write!( self.buf, "\"{prop_name}\":");
        self.buf.write_char('[');
        f (self);
        self.buf.write_char(']');
    }
    
    pub fn write_fmt_field (&mut self, prop_name: &str, value: &str) {
        self.check_separator();
        write!( self.buf, "\"{prop_name}\":");
        write!( self.buf, "{value}");
    }

    // let the provided closure determine how to write the value
    pub fn write_with (&mut self, f: impl FnOnce(&mut JsonWriter)) {
        self.check_separator();
        f(self)
    }

    // let the provided closure determine how to write the field
    pub fn write_field_with (&mut self, prop_name: &str, f: impl FnOnce(&mut JsonWriter)) {
        self.check_separator();
        write!( self.buf, "\"{prop_name}\":");
        f(self)
    }
    
    /// this is a catch-all for proper string/number formatting
    pub fn write_field<T:Debug> (&mut self, prop_name: &str, value: T) {
        self.check_separator();
        write!( self.buf, "\"{prop_name}\":");
        write!( self.buf, "{:?}", value);
    }
    
    pub fn write_json_field<T:JsonWritable> (&mut self, prop_name: &str, value: &T) {
        self.check_separator();
        write!( self.buf, "\"{prop_name}\":");
        value.write_json_to(self);
    }
    
    pub fn write_value<T:Debug> (&mut self, value: T) {
        self.check_separator();
        write!( self.buf, "{value:?}");
    }
    
    

    pub fn to_string(self)->String { self.buf }
    
    pub fn len (&self)->usize {
        self.buf.len()
    }

    pub fn is_empty (&self)->bool {
        self.buf.is_empty()
    }

    #[inline] fn check_separator (&mut self) {
        if let Some(b) = self.last_byte() {
            if b != b'{' && b != b'[' && b != b',' && b != b':' {
                self.buf.write_char(',');
            }
        }
    }

    fn last_byte (&self)->Option<u8> {
        let bs = self.buf.as_bytes();
        let len = bs.len();
        if len > 0 {
            Some(bs[len-1])
        } else {
            None
        }
    }
}

impl From<JsonWriter> for String {
    fn from(w: JsonWriter)->String { w.buf }
}

pub trait JsonWritable {
    /// note - this has to include brackets ("{..}" or "[..]" if this is a container)
    fn write_json_to (&self, w: &mut JsonWriter);

    fn as_json (&self)->String {
        let mut w = JsonWriter::with_capacity( self.estimated_length());
        self.write_json_to(&mut w);
        w.to_string()
    }

    fn estimated_length (&self)->usize {
        256
    }
}

impl <T: JsonWritable> JsonWritable for &[T] {
    fn write_json_to (&self, w: &mut JsonWriter) {
        w.write_array(|w| {
            for e in self.iter() {
                e.write_json_to(w)
            }
        });
    }
} 

impl <T: JsonWritable> JsonWritable for Vec<T> {
    fn write_json_to (&self, w: &mut JsonWriter) {
        self.as_slice().write_json_to(w);
    }
} 
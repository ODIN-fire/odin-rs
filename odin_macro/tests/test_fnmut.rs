/*
 * Copyright (c) 2024, United States Government, as represented by the
 * Administrator of the National Aeronautics and Space Administration.
 * All rights reserved.
 *
 * The ODIN - Open Data Integration Framework is licensed under the
 * Apache License, Version 2.0 (the "License"); you may not use this file
 * except in compliance with the License. You may obtain a copy of the
 * License at http://www.apache.org/licenses/LICENSE-2.0.
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

use odin_macro::fn_mut;

#[test]
fn test_fnmut() {
    let foo = "foo".to_string();

    let mut f1 = fn_mut!{
        (mut foo=foo.clone(), b=foo.len()) => |x: usize| {
            foo.push_str(" boo");
            println!("f1: {foo} from {b} and {x:?}");
        }
    };
    f1(42);
    
    let f2 = fn_mut!( (foo=foo.clone()) => println!("f2: {foo}"));
    f2();
    
    let f3 = fn_mut!( |a| println!("f3: {a:?}"));
    f3(&foo);
    
    let f4 = fn_mut!{ println!("f4: {foo}")};
    f4();
}
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

/// heap related functions, including debug support for dhat-rs
/// use the online Valgrind [DHAT Viewer](https://nnethercote.github.io/dh_view/dh_view.html) to look at the produces `dhat-heap.json` file

/// this has to be called at top of the main module (outside main)
#[macro_export]
macro_rules! use_dhat {
    () => {
        #[cfg(feature="dhat")]
        use dhat;

        #[cfg(feature = "dhat")]
        #[global_allocator]
        static ALLOC: dhat::Alloc = dhat::Alloc;
    }
}
pub use use_dhat;

/// to be called at top of main func (introduces scope guard)
/// note this also requires to set graceful ctrlc exit (e.g.  process::set_ctrlc_handler ) if that is how to 
/// terminate the binary
#[macro_export]
macro_rules! init_dhat {
    () => {
        #[cfg(feature = "dhat")]
        let __profiler__ = dhat::Profiler::new_heap();

        #[cfg(feature = "dhat")]
        println!("writing 'dhat-heap.json' (analyze with https://nnethercote.github.io/dh_view/dh_view.html)");

        #[cfg(not(feature = "dhat"))]
        println!("warning - executable was not built with \"... --features dhat ...\" - no DHAT output will be produced");
    }
}
pub use init_dhat;
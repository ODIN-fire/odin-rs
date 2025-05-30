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
#![allow(unused)]
///! This crate provides procedural macros used throughout the ODIN project, namely
///!
///!    - [`define_algebraic_type`] and [`match_algebraic_type`]
///!    - [`define_actor_msg_type`] and [`match_actor_msg`] (the [`odin_actor`] specific versions)
///!    - [`impl_actor`] and [`spawn_actor`]
///! 
///! Its main use case within ODIN is to support concise syntax for [`odin_actor::Actor`] implementation as in:
///! ```
///!     define_actor_msg_type! {
///!         MyActorMsg = A | B<std::vec::Vec<(u32,&'static str)>>
///!     }
///!     struct MyActorState {...}
///!     ...
///!     impl_actor! { match msg for Actor<MyActorState,MyActorMsg> {
///!         B<std::vec::Vec<(u32,&'static str)>> => cont! { println!("got a B: {:?}", msg) }
///!         ...
///!     }
///! ```
// TODO - still needs optional generic_params and where_clause

extern crate proc_macro;

use proc_macro::TokenStream;
use proc_macro2::{
	Literal, Punct, Spacing, Span, TokenStream as TokenStream2, TokenTree
};
use quote::{format_ident, quote, ToTokens, TokenStreamExt};
use syn::{ 
	self, parse::{Lookahead1, Parse, ParseStream, Result, Nothing}, parse_quote, 
    parse_macro_input, punctuated::{Punctuated}, visit::{self, Visit}, 
    token::{self, Mut, Ref, Where, Colon, Gt, Lt, Comma, Paren, PathSep, Use, For, In}, 
    Attribute, Block, Expr, ExprLit, ExprCall, ExprBlock, ExprMacro, ExprMethodCall, FnArg, Ident, ItemEnum, ItemFn, ItemStruct, Path, PathSegment, 
    PredicateType, Stmt, Token, Type, TypePath, Visibility, WhereClause, WherePredicate, GenericParam, PathArguments,
    parenthesized
};
use std::{collections::HashSet,str::FromStr};

macro_rules! stringify_path {
    ( $path:path ) => {
        stringify!($path)
    }
}
/* #region define_struct **************************************************************/

/// convenience macro to generate a struct definition with respective default constructor
/// 
/// example:
/// ```rust
/// define_struct! {
///     pub MyStruct<A>: Debug + Clone where A: Foo + Debug =
///         field_1: A,                                 // no init expr -> becomes ctor arg
///         field_2: Vec<String>   = Vec::new(),        // func call init
///         field_3: &'static str  = "blah",            // literal init
///         field_4: usize         = { field_3.len() }  // block init (with back-ref)
/// }
/// ```
/// is expanded into:
/// ```rust
/// #[derive(Debug,Clone)]
/// pub struct MyStruct<A> where A: Foo + Debug {
///     field_1: A,
///     ...
/// }
/// impl <A> MyStruct<A> where A: Foo + Debug {
///     pub fn new (field_1: A)->Self {
///         let field_2: Vec<String> = Vec::new();
///         let field_3: &'static str = "blah";
///         let field_4: usize = { field_3.len() };
///         MyStruct { field_1, field_2, field_3, field_4 }
///     } 
/// }
/// ```
/// 
#[proc_macro]
pub fn define_struct (item: TokenStream) -> TokenStream {
    let StructSpec{ attrs, visibility, name, generic_params, derives, where_clause, field_specs } = match syn::parse(item) {
        Ok(struct_spec) => struct_spec,
        Err(e) => panic!( "expected \"structName [: Trait,..] = fieldSpec, ..\" got error: {:?}", e)
    };
    let generics = if generic_params.is_empty() { quote!{} } else { quote! { < #( #generic_params ),* > } };
    let derive_clause = if derives.is_empty() { quote!{} } else { quote! { #[derive( #( #derives ),* )] } };
    let inherent_impl = get_inherent_impl( &visibility, &name, &generic_params, &where_clause,  &field_specs);

    let new_item: TokenStream = quote! {
        #derive_clause
        #( #attrs )*
        #visibility struct #name #generics #where_clause {
            #( #field_specs ),*
        }
        #inherent_impl
    }.into();
    //println!("-----\n{}\n-----", new_item.to_string());
    new_item
}

fn get_inherent_impl (visibility: &Visibility, name: &Ident, generic_params: &Vec<GenericParam>, where_clause: &Option<WhereClause>, field_specs: &Vec<FieldSpec>)->TokenStream2 {
    let ctor_arg_list: TokenStream2 = get_ctor_arg_list( field_specs);
    let mut generic_names = get_generic_names(generic_params);
    let generics = if generic_params.is_empty() { quote!{} } else { quote! { < #( #generic_params ),* > } };
    let field_names: Vec<&Ident> = field_specs.iter().map( |f| &f.name).collect();
    let init_stmts: TokenStream2 = get_ctor_init_stmts( field_specs);

    quote!{
        impl #generic_names #name #generics #where_clause {
            #visibility fn new ( #ctor_arg_list )->Self {
                #init_stmts
                #name { #( #field_names ),* }
            }
        }
    }
}

fn get_generic_names (generic_params: &Vec<GenericParam>)->TokenStream2 {
    let mut ts = TokenStream2::new();

    if !generic_params.is_empty() {
        let mut is_first = true;
        ts.append( Punct::new('<', Spacing::Alone));
        for g in generic_params.iter() {
            if !is_first {
                ts.append( Punct::new(',', Spacing::Alone));
            } else { 
                is_first = false;
            }

            match g {
                GenericParam::Type(g) => ts.append( g.ident.clone()),
                GenericParam::Lifetime(g) => {
                    ts.append( Punct::new('\'', Spacing::Joint));
                    ts.append( g.lifetime.ident.clone());
                }
                GenericParam::Const(g) => ts.append( g.ident.clone())
            }
        }
        ts.append( Punct::new('>', Spacing::Alone));
    }

    ts
}

fn get_ctor_arg_list (field_specs: &Vec<FieldSpec>)->TokenStream2 {
    let no_init_args: Vec<TokenStream2> = field_specs.iter().filter(|f| f.init_expr.is_none()).map(|f|{
        let ident = &f.name;
        let field_type = &f.field_type;
        quote! { #ident : #field_type }
    }).collect();

    if !field_specs.is_empty() {
        quote!{ #( #no_init_args ),* }
    } else {
        quote!{}
    }
}

fn get_ctor_init_stmts (field_specs: &Vec<FieldSpec>)->TokenStream2 {
    let mut ts = TokenStream2::new();
    for f in field_specs {
        if let Some(init) = &f.init_expr {
            let name = &f.name;
            let ftype = &f.field_type;
            ts.append_all( quote!{ let #name : #ftype = #init; } );
        }
    }
    ts
}

struct StructSpec {
    attrs: Vec<Attribute>,
    visibility: Visibility,
    name: Ident,
    generic_params:Vec<GenericParam>,
    derives: Vec<Path>,
    where_clause: Option<WhereClause>,
    field_specs: Vec<FieldSpec>,
}

impl Parse for StructSpec {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let attrs: Vec<Attribute> = input.call(Attribute::parse_outer)?;
        let visibility: Visibility = parse_visibility(input);
        let name: Ident = input.parse()?;

        let mut generic_params: Vec<GenericParam> = Vec::new();
        let mut lookahead = input.lookahead1();
        if !input.is_empty() && lookahead.peek(Token![<]) {
            input.parse::<Token![<]>()?;
            generic_params = Punctuated::<GenericParam,Token![,]>::parse_separated_nonempty(input)?.into_iter().collect();
            input.parse::<Token![>]>()?;
            lookahead = input.lookahead1();
        }

        let mut derives: Vec<Path> = Vec::new();
        if !input.is_empty() && lookahead.peek(Token![:]) {
            input.parse::<Token![:]>()?;
            derives = Punctuated::<Path,Token![+]>::parse_separated_nonempty(input)?.into_iter().collect();
            lookahead = input.lookahead1();
        }

        let mut where_clause: Option<WhereClause> = None;
        if !input.is_empty() && lookahead.peek(Token![where]) {
            where_clause = Some(input.parse::<WhereClause>()?);
            lookahead = input.lookahead1();
        }

        let mut field_specs: Vec<FieldSpec> = Vec::new();
        if !input.is_empty() && lookahead.peek(Token![=]) {
            input.parse::<Token![=]>()?;
            field_specs = Punctuated::<FieldSpec,Token![,]>::parse_separated_nonempty(input)?.into_iter().collect();
        }

        Ok(StructSpec { attrs, visibility, name, generic_params, derives, where_clause, field_specs })
    }
}


struct FieldSpec {
    attrs: Vec<Attribute>,
    visibility: Visibility,
    name: Ident,
    colon_token: Colon,
    field_type: Type,
    init_expr: Option<Expr>,
}

impl Parse for FieldSpec {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let attrs: Vec<Attribute> = input.call(Attribute::parse_outer)?;
        let visibility: Visibility = parse_visibility(input);
        let name: Ident = input.parse()?;
        let colon_token: Colon = input.parse::<Token![:]>()?;
        let field_type: Type = input.parse::<Type>()?;

        let mut lookahead = input.lookahead1();
        let mut init_spec: Option<Expr> = None;
        if !input.is_empty() && lookahead.peek(Token![=]) {
            input.parse::<Token![=]>()?;
            init_spec = Some(input.parse::<Expr>()?);
        }

        Ok( FieldSpec{ attrs, visibility, name, colon_token, field_type, init_expr: init_spec })
    }
}

impl ToTokens for FieldSpec {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        for a in &self.attrs { a.to_tokens(tokens); }
        self.visibility.to_tokens(tokens);
        self.name.to_tokens(tokens);
        self.colon_token.to_tokens(tokens);
        self.field_type.to_tokens(tokens);
    }
}

/* #endregion define_struct */

/* #region define_algebraic_type ******************************************************/

/// macro to define algebraic types (using a Haskell'ish syntax), which are mapped into enums
/// whose variant names are transparent (automatically generated from element types).
/// 
/// variant names are computed from their respective types and are implemented as simple 1-element
/// tuple structs. The encoding uses unicode characters that resemble type tokens ('<' etc) but
/// are not likely to be used in normal code. We choose readability over typing since the user
/// is not supposed to enter those variant names manually and use the [`match_algebraic_type`] macro
/// instead (which uses the same encoding of types).
/// 
/// Note: if message variants use path types (e.g. `std::vec::Vec`) the same notation
/// has to be used in both [`define_algebraic_type`] and [`match_algebraic_type`] 
/// 
/// The macro supports an optional derive clause
/// ```
///     define_algebraic_type! { MyEnum: Trait1,... = ... }
/// ```
/// that is expanded into a respective `#[derive(Trait1,..)` macro for the resulting enum.
/// 
/// As a convenience feature it also supports optional method definitions that are expanded for
/// all variants if their bodies include `__` (double underscore) as variable names. If present these
/// methods are turned into an inherent impl for the enum. 
/// 
/// Example:
/// ```
/// struct A { id: u64 }
/// struct B<T> { id: u64, v: T }
/// 
/// define_algebraic_type! {
///     pub MyMsg: Clone = A | B<std::vec::Vec<(u32,&'static str)>>
///     pub fn id(&self)->u64 { __.id }
///     pub fn description()->'static str { "my message enum" }
/// }
/// ```
/// This is expanded into
/// ```
/// #[derive(Debug)]
/// #[derive(Clone)]
/// pub enum MyMsg {
///     A (A),
///     BᐸstdːːvecːːVecᐸ𛰙u32ˎᴿʽstaticˑstr𛰚ᐳᐳ (B<std::vec::Vec<(u32,&'static str)>>),
/// }
/// impl MyMsg {
///     pub fn id(&self)->u64 {
///         match self {
///             Self::A (__) => { __.id }
///             Self::BᐸstdːːvecːːVecᐸ𛰙u32ˎᴿʽstaticˑstr𛰚ᐳᐳ (__) => { _.id }
///         }
///     }
/// }
/// impl From<A> for MyMsg {...}
/// impl From<B<std::vec::Vec<(u32,&'static str)>>> for MyMsg {...}
/// ```
#[proc_macro]
pub fn define_algebraic_type (item: TokenStream) -> TokenStream {
    let AdtEnum {attrs, visibility, name, generic_params, derives, where_clause, variant_types, methods }= match syn::parse(item) {
        Ok(adt) => adt,
        Err(e) => panic!( "expected \"adtName [: Trait,..] = variantType | ..  [ func ... ]\" got error: {:?}", e)
    };

    let mut variant_names = get_variant_names_from_types(&variant_types);

    let mut generic_names = get_generic_names( &generic_params);
    let generics = if generic_params.is_empty() { quote!{} } else { quote! { < #( #generic_params ),* > } };

    let derive_clause = if derives.is_empty() { quote!{} } else { quote! { #[derive( #( #derives ),* )] } };
    let inherent_impl = if methods.is_empty() { quote!{} } else { build_inherent_impl( &name, &generic_names, &generics, &where_clause, &variant_names, &methods) };

    let new_item: TokenStream = quote! {
        #derive_clause
        #( #attrs )*
        #visibility enum #name #generics #where_clause {
            #( #variant_names ( #variant_types ) ),*
        }
        #inherent_impl
        #(
            impl #generic_names From<#variant_types> for #name #generics #where_clause {
                fn from (v: #variant_types)->Self { #name::#variant_names(v) }
            }
        )*
        impl #generic_names std::fmt::Debug for #name #generics #where_clause {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self {
                    #( Self::#variant_names (msg) => write!(f, concat!( stringify!(#name), "::", stringify!(#variant_names))) ),*
                }
            }
        }
    }.into();
    //println!("-----\n{}\n-----", new_item.to_string());
    new_item
}

fn build_inherent_impl (enum_name: &Ident, generic_names: &TokenStream2, generics: &TokenStream2, 
                        where_clause: &Option<WhereClause>, variant_names: &Vec<Ident>, methods: &Vec<ItemFn>)->TokenStream2 {
    let mthds: Vec<TokenStream2> = methods.iter().map( |m| build_enum_method( variant_names, m)).collect();

    quote! {
        impl #generic_names #enum_name #generics #where_clause {
            #( #mthds )*
        }
    }
}

fn build_enum_method (variant_names: &Vec<Ident>, method: &ItemFn)->TokenStream2 {
    let vis = &method.vis;
    let sig = &method.sig;
    let blk = &method.block;

    let mut block_analyzer = BlockAnalyzer::new();
    block_analyzer.visit_block(blk);

    if block_analyzer.uses_variant { // expand for all variants
        // variant name placeholders:
        // unfortunately we cannot use a single '_' wildcard since it is not a normal ident and cannot be replaced easily
        // we also might use normal wildcards in the function body.
        // the next best choice is '__', which is actually a valid ident and rarely used 

        quote! {
            #vis #sig {
                match self {
                    #( Self::#variant_names ( __ ) => #blk )*
                }
            }
        }
    } else { // expand verbatim
        quote! {
            #vis #sig #blk
        }
    }
}

struct BlockAnalyzer { uses_variant: bool }
impl BlockAnalyzer {
    fn new()->Self { BlockAnalyzer { uses_variant: false } }
}

impl<'a> Visit<'a> for BlockAnalyzer {
    fn visit_ident(&mut self, ident: &'a Ident) {
        if ident.to_string() == "__" { 
            self.uses_variant = true;
        }
        visit::visit_ident(self, ident)
    }
}

/* #endregion define_algebraic_type */

/* #region define_actor_msg_type ***********************************************************/

/// the odin_actor specific version of the general [`define_algebraic_type`] macro.
/// this automatically adds system messages (_Start_,_Terminate_,..) variants and
/// a [`odin_actor::DefaultReceiveAction`]` impl.
/// 
/// Example:
/// ```
/// define_actor_msg_set! { pub MyActorMsg = A | B }
/// ```
/// This is expanded into
/// ```
/// #[derive(Debug)]
/// pub enum MyActorMsg {
///     A (A),
///     B (B),
///     _Start_ (_Start_), ... _Terminate_ (_Terminate_)
/// }
/// impl FromSysMsg for MyActorMsg {...}
/// impl From<A> for MyActorMsg {...}
/// impl From<B> for MyActorMsg {...}
/// impl DefaultReceiveAction for MyActorMsg {...}
/// 
#[proc_macro]
pub fn define_actor_msg_set (item: TokenStream) -> TokenStream {
    let AdtEnum {attrs, visibility, name, generic_params, derives, where_clause, mut variant_types, methods }= syn::parse(item).unwrap();
    for var_type in get_sys_msg_types() {
        variant_types.push(var_type)
    }

    let mut variant_names = get_variant_names_from_types(&variant_types);
    for var_name in get_sys_msg_idents() {
        variant_names.push(var_name)
    }

    let mut generic_names = get_generic_names( &generic_params);
    let generics = if generic_params.is_empty() { quote!{} } else { quote! { < #( #generic_params ),* > } };

    let derive_clause = if derives.is_empty() { quote!{} } else { quote! { #[derive( #( #derives ),* )] } };
    let inherent_impl = if methods.is_empty() { quote!{} } else { build_inherent_impl( &name, &generic_names, &generics, &where_clause, &variant_names, &methods) };

    let new_item: TokenStream = quote! {
        #derive_clause
        #( #attrs )*
        #visibility enum #name #generics #where_clause {
            #( #variant_names ( #variant_types ) ),*
        }

        #inherent_impl
        impl #generic_names FromSysMsg for #name #generics #where_clause {}
        #(
            impl #generic_names From<#variant_types> for #name #generics #where_clause {
                fn from (v: #variant_types)->Self { #name::#variant_names(v) }
            }
        )*
        impl #generic_names std::fmt::Debug for #name #generics #where_clause {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self {
                    #( #name::#variant_names (msg) => write!(f, "{:?}", msg), )*
                }
            }
        }
        impl #generic_names DefaultReceiveAction for #name #generics #where_clause {
            fn default_receive_action (&self)->ReceiveAction {
                match self {
                    #name::_Exec_(msg) => { msg.0(); ReceiveAction::Continue }
                    #name::_Ping_(msg) => { msg.store_response(); ReceiveAction::Continue }
                    #name::_Terminate_(msg) => ReceiveAction::Stop,
                    _ => ReceiveAction::Continue
                }
            }
        }

    }.into();
    //println!("-----\n{}\n-----", new_item.to_string());

    new_item
}

fn get_variant_names_from_types (variant_types: &Vec<Path>)->Vec<Ident> {
    variant_types.iter().map( |p| {
        let ps = path_to_string( p);
        let ps_mangled = mangle(ps.as_str());
        Ident::new( &ps_mangled.as_str(), Span::call_site())
    }).collect()
}

#[derive(Debug)]
struct AdtEnum {
    attrs: Vec<Attribute>,
    visibility: Visibility,
    name: Ident,
    generic_params:Vec<GenericParam>,
    derives: Vec<Path>,
    where_clause: Option<WhereClause>,
    variant_types: Vec<Path>,
    methods: Vec<ItemFn>
}

impl Parse for AdtEnum {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let attrs: Vec<Attribute> = input.call(Attribute::parse_outer)?;
        let visibility: Visibility = parse_visibility(input);
        let name: Ident = input.parse()?;

        let mut generic_params: Vec<GenericParam> = Vec::new();
        let mut lookahead = input.lookahead1();
        if !input.is_empty() && lookahead.peek(Token![<]) {
            input.parse::<Token![<]>()?;
            generic_params = Punctuated::<GenericParam,Token![,]>::parse_separated_nonempty(input)?.into_iter().collect();
            input.parse::<Token![>]>()?;
            lookahead = input.lookahead1();
        }

        let mut derives: Vec<Path> = Vec::new();
        let mut lookahead = input.lookahead1();
        if !input.is_empty() && lookahead.peek(Token![:]) {
            input.parse::<Token![:]>()?;
            derives = Punctuated::<Path,Token![+]>::parse_separated_nonempty(input)?.into_iter().collect();
            lookahead = input.lookahead1();
        }

        let mut where_clause: Option<WhereClause> = None;
        if !input.is_empty() && lookahead.peek(Token![where]) {
            where_clause = Some(input.parse::<WhereClause>()?);
            lookahead = input.lookahead1();
        }

        let variant_types: Vec<Path> = if !input.is_empty() && lookahead.peek(Token![=]) {
            input.parse::<Token![=]>()?;
            let variant_types = Punctuated::<Path,Token![|]>::parse_separated_nonempty(input)?;
            variant_types.into_iter().collect()
        } else {
            Vec::new()
        };
        
        let mut methods: Vec<ItemFn> = Vec::new();
        lookahead = input.lookahead1();
        while !input.is_empty() && (lookahead.peek(Token![fn]) || lookahead.peek(Token![pub])) {
            let mth: ItemFn = input.parse()?;
            methods.push(mth);
            lookahead = input.lookahead1()
        }

        Ok( AdtEnum { attrs, visibility, name, generic_params, derives, where_clause, variant_types, methods })
    }
}

/* #endregion define_actor_msg_type */

/* #region match macros **********************************************************/

/// macro to match algebraic type values (enum variants) that were created with the
/// [`define_algebraic_type`] macro
/// Example:
/// ```
/// define_algebraic_type!( MyType = A | B<C,D> | E)
/// ...
/// match_algebraic_type! { my_type: MyType as
///   A => cont { println!("got an A : {}",my_type); }
///   x::B<C,D> => { println!("got a B<C,D>"); }
///   E => { println!("got an E") }
/// }
/// ```
#[proc_macro]
pub fn match_algebraic_type (item: TokenStream) -> TokenStream {
    let MsgMatch { msg_name, msg_type, match_arms }: MsgMatch = match syn::parse(item) {
        Ok(msg_match) => msg_match,
        Err(e) => panic!( "expected \"match_algebraic_type!( «msgVarName»:«EnumType» as «VariantType» => {{..}}, ..)\", got {:?}", e)
    };

    let match_patterns: Vec<TokenStream2> = get_match_patterns(&msg_name, &msg_type, &match_arms);
    let match_actions: Vec<&Expr> = match_arms.iter().map( |a| { &a.match_action }).collect();

    let new_item: TokenStream = quote! {
        match #msg_name {
            #(
                #match_patterns => #match_actions
            ),*
        }
    }.into();
    //println!("-----\n{}\n-----", new_item.to_string());

    new_item
}

fn get_match_patterns(msg_name: &Ident, msg_type: &Path, match_arms: &Vec<MsgMatchArm>)->Vec<TokenStream2> {
    let match_msg_type = get_match_adt_type( msg_type);

    match_arms.iter().map(|a| {
        match &a.variant_spec {
            VariantSpec::Type(path) => {
                let variant_name = get_variant_name_from_match_arm(a);
                let maybe_ref = a.maybe_ref;
                let maybe_mut = a.maybe_mut;

                quote!( 
                    #match_msg_type::#variant_name (#maybe_ref #maybe_mut #msg_name)
                )
            }
            VariantSpec::Wildcard => { quote!(_) }
        }
    }).collect()
}

/// the odin_actor specific version of the general [`match_algebraic_type`] macro.
/// this automatically adds system message (_Start_,_Terminate_,..) variants and
/// a default match arm that calls `msg.default_receive_action()`.
/// 
/// Match arm actions can use the [`cont`], [`stop`] and [`term`] macros to return
/// respective [`odin_actor::ReceiveAction`] values
/// 
/// Note: if message variants use path types (e.g. `std::vec::Vec`) the same notation
/// has to be used in both [`define_actor_msg_type`] and [`match_actor_msg_type`] 
/// 
/// Example:
/// ```
///     define_actor_msg_type! { MyActorMsg = x::A | B }
///     ...
///     match_actor_msg_type! { msg: MyActorMsg as 
///         x::A => cont! { println!("actor received an A = {:?}", msg) }
///         _Terminate_ => stop! { println!("actor terminated") }
///     }
/// ```
/// This is expanded into:
/// ```
///     match msg {
///        xːːA(msg) => { {println!("actor received an A = {:?}", msg)}; ReceiveAction::Continue }
///        _Terminate_(msg) => { {println!("actor terminated {:?}", msg)}; ReceiveAction::Stop }
///        _ => msg.default_receive_action()
///     }
/// ```
/// 
#[proc_macro]
pub fn match_actor_msg (item: TokenStream)->TokenStream {
    let MsgMatch { msg_name, msg_type, match_arms }: MsgMatch = syn::parse(item).unwrap();
    
    let variant_names: Vec<Ident> = get_variant_names_from_match_arms(&match_arms);
    let is_mut: Vec<&Option<Token![mut]>> = match_arms.iter().map( |a| { &a.maybe_mut }).collect();
    let match_msg_type = get_match_adt_type( &msg_type);
    let match_actions: Vec<&Expr> = match_arms.iter().map( |a| { &a.match_action }).collect();

    let new_item: TokenStream = quote! {
        match #msg_name {
            #( #match_msg_type::#variant_names (#is_mut #msg_name) => #match_actions, )*

            // this relies on Rust allowing duplicated match patterns and ignoring all but the first
            #match_msg_type::_Start_(_) => msg.default_receive_action(),
            #match_msg_type::_Ping_(_) => msg.default_receive_action(),
            #match_msg_type::_Timer_(_) => msg.default_receive_action(),
            #match_msg_type::_Exec_(_) => msg.default_receive_action(),
            #match_msg_type::_Pause_(_) => msg.default_receive_action(),
            #match_msg_type::_Resume_(_) => msg.default_receive_action(),
            #match_msg_type::_Terminate_(_) => msg.default_receive_action(),
            //_ => #msg_name . default_receive_action() // this would be a catch-all which would bypass the check for unmatched user messages
        }
    }.into();
    //println!("-----\n{}\n-----", new_item.to_string());

    new_item
}

fn get_variant_name_from_match_arm (a: &MsgMatchArm)->Ident {
    let ps = variant_spec_to_string( &a.variant_spec);
    let ps_mangled = mangle(ps.as_str());
    Ident::new( &ps_mangled.as_str(), Span::call_site())
}

fn get_variant_names_from_match_arms (match_arms: &Vec<MsgMatchArm>)->Vec<Ident> {
    match_arms.iter().map( |a| get_variant_name_from_match_arm(a)).collect()
}

fn get_variant_types_from_match_arms (match_arms: &Vec<MsgMatchArm>)->Vec<Path> {
    let mut var_types: Vec<Path> = Vec::new();

    for a in match_arms {
        match &a.variant_spec {
            VariantSpec::Type(path) => {
                if let Some(last_seg) = path.segments.last() {
                    let name = last_seg.ident.to_string();
                    if !(name.starts_with("_") && name.ends_with("_")) {
                        var_types.push(path.clone())
                    }
                }
            }
            _ => {} // we are not interested in wildcards
        }
    }

    /* here we could add system messages but those should only be sent through the SysMsgReceiver trait, i.e. from the ActorSystem
    var_types.push( sys_msg_path("_Start_"));
    var_types.push( sys_msg_path("_Ping_"));
    var_types.push( sys_msg_path("_Timer_"));
    var_types.push( sys_msg_path("_Pause_"));
    var_types.push( sys_msg_path("_Resume_"));
    var_types.push( sys_msg_path("_Terminate_"));
    */

    var_types

}

fn sys_msg_path (name: &'static str)->Path {
    let crate_ident = Ident::new("odin_actor", Span::call_site());
    let ident = Ident::new( name, Span::call_site());
    let mut segments: Punctuated<PathSegment,PathSep> = Punctuated::new();
    segments.push( PathSegment { ident: crate_ident, arguments: PathArguments::None });
    segments.push( PathSegment{ ident, arguments: PathArguments::None });

    Path{ leading_colon: None, segments }
}

struct MsgMatch {
    msg_name: Ident, // the msg variable name to bind
    msg_type: Path, // the msg type to match
    match_arms: Vec<MsgMatchArm>
}

struct MsgMatchArm {
    variant_spec: VariantSpec,
    maybe_ref: Option<Token![ref]>,
    maybe_mut: Option<Token![mut]>,
    match_action: Expr,
}

enum VariantSpec {
    Type(Path),
    Wildcard
}

impl Parse for MsgMatch {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let msg_name: Ident = input.parse()?;
        let _: Token![:] = input.parse()?;
        let msg_type: Path = input.parse()?;
        let _: Token![as] = input.parse()?;
        let match_arms = parse_match_arms(input)?;

        Ok( MsgMatch { msg_name, msg_type, match_arms } )
    }
}

// TODO - this should be consistent over all our ADTs
fn parse_match_arms (input: ParseStream)->Result<Vec::<MsgMatchArm>> {
    let mut match_arms = Vec::<MsgMatchArm>::new();
    
    while !input.is_empty() {
        let lookahead = input.lookahead1();
        let (variant_spec,is_ref,is_mut) = if lookahead.peek( Token![_]) {
            let _: Token![_] = input.parse()?;
            (VariantSpec::Wildcard,Option::<Ref>::None,Option::<Mut>::None)
        } else {
            let is_ref: Option<Token![ref]> = if lookahead.peek( Token![ref]) {
                Some(input.parse()?)
            } else { None };

            let is_mut: Option<Token![mut]> = if lookahead.peek( Token![mut]) {
                Some(input.parse()?)
            } else { None };
    
            let path: Path = input.parse()?;
            (VariantSpec::Type(path),is_ref,is_mut)
        };
        
        //--- the match 
        let _: Token![=>] = input.parse()?;
        let match_action: Expr = input.parse()?;

        let lookahead = input.lookahead1();
        if lookahead.peek(Token![,]) { // FIXME - does not work! 
            let _: Token![,] = input.parse()?;
        }

        match_arms.push( MsgMatchArm { variant_spec, maybe_ref: is_ref, maybe_mut: is_mut, match_action } );
    }

    Ok(match_arms)
}

/// if the ADT type has generic params we have to remove these from match arm expressions:
/// ```
///   enum MyAdt<T> { Foo(T), ... }
/// ```
/// is matched like this:
/// ```
///   match adt {
///      MyAdt::Foo(t) => ...
///   }
/// ```
fn get_match_adt_type (adt_type: &Path)->Path {
    let mut match_adt_type = adt_type.clone();
    let mut mt = match_adt_type.segments.last_mut().unwrap();
    mt.arguments = PathArguments::None;

    match_adt_type
}

/* #endregion match macros */

/* #region actor receive definition ****************************************************************/

/// defines the message related behavior of an actor by creating an [`ActorReceiver`] impl from the provided spec  
/// 
/// Example:
/// ```
/// impl_actor! { match msg for Actor<MyActor,MyActorMsg> as
///     _Start_ => ... // do whatever needs to be done for system _Start_ message
///     Query<Question,Answer> => ... // reply to query
/// }
/// ```
/// which gets translated into:
/// ```
/// impl ActorReceiver for Actor<MyActor,MyActorMsg> {
///   fn receive (&mut self, msg: MyActorMsg)->ReceiveAction {
///      MyActorMsg::_Start_ => ...
///      MyActorMsg::Query => ...  // the real variant name gets mangled from the provided type
///   }
/// }
/// ```
#[proc_macro]
pub fn impl_actor (item: TokenStream) -> TokenStream {
    let ActorReceive { msg_name, mut msg_type, state_type, where_clause, match_arms }: ActorReceive = match syn::parse(item) {
        Ok(actor_receive) => actor_receive,
        Err(e) => panic!( "expected impl_actor!{{ match «msgVarName» for Actor<«stateType»,«msgType»> [where ..] as «msgTypeVariant» => {{..}},...}}, got {:?}", e)
    };

    let match_msg_type = get_match_adt_type( &msg_type);

    let variant_names: Vec<Ident> = get_variant_names_from_match_arms(&match_arms);
    //let variant_types: Vec<Path> = get_variant_types_from_match_arms(&match_arms); // if we need to do explicit trait impls for variant types
    let is_mut: Vec<&Option<Token![mut]>> = match_arms.iter().map( |a| { &a.maybe_mut }).collect();
    let match_actions: Vec<&Expr> = match_arms.iter().map( |a| { &a.match_action }).collect();

    let typevars: Vec<&Path> = if let Some(ref wc) = where_clause { collect_typevars( wc) } else { Vec::new() }; 
    let typevar_tokens: TokenStream2 = if typevars.is_empty() { quote! {} } else {
        quote! { < #( #typevars ),* > }
    };

    let new_item: TokenStream = quote! {
        impl #typevar_tokens ActorReceiver<#msg_type> for Actor<#state_type,#msg_type> #where_clause {
            async fn receive (&mut self, #msg_name: #msg_type)->ReceiveAction {
                #[allow(unused_variables)] // some match arms might not use msg_name
                match #msg_name {
                    #( #match_msg_type::#variant_names (#is_mut #msg_name) => #match_actions, )*

                    // this relies on Rust allowing duplicated match patterns and ignoring all but the first matching arm
                    #match_msg_type::_Start_(_) => #msg_name.default_receive_action(),
                    #match_msg_type::_Ping_(_) => #msg_name.default_receive_action(),
                    #match_msg_type::_Timer_(_) => #msg_name.default_receive_action(),
                    #match_msg_type::_Exec_(_) => #msg_name.default_receive_action(),
                    #match_msg_type::_Pause_(_) => #msg_name.default_receive_action(),
                    #match_msg_type::_Resume_(_) => #msg_name.default_receive_action(),
                    #match_msg_type::_Terminate_(_) => #msg_name.default_receive_action(),
                    //_ => #msg_name . default_receive_action() // this would be a catch-all which would cut off the check for unmatched user messages
                }
            }
            fn hsys(&self)->&ActorSystemHandle { self.hself.hsys() }
        }
        /* explicit trait impl for variant types would go here 
        #( 
            impl MsgReceiver< #variant_types > for ActorHandle< #msg_type > {
                fn send_msg<'a> (&'a self, m: #variant_types)->SendMsgFuture<'a> { self.send_actor_msg(m.into()) }
                fn move_send_msg (self, m: MsgType)->MoveSendMsgFuture { self.move_send_msg(m.into()) }
                fn timeout_send_msg<'a> (&self, m: MsgType, to: Duration)->TimeoutSendMsgFuture<'a> { self.timeout_send_actor_msg( m.into(), to) }
                fn timeout_move_send_msg (self, m: MsgType, to: Duration)->TimeoutMoveSendMsgFuture { self.timeout_move_send_msg( m, to) }
                fn try_send_msg (&self, m:MsgType)->Result<()> { self.try_send_actor_msg(m) }
            }
        )*
        */
    }.into();
    //println!("-----\n{}\n-----", new_item.to_string());

    new_item
}

struct ActorReceive {
    msg_name: Ident,
    msg_type: Path,
    state_type: Path,
    where_clause: Option<WhereClause>, 
    match_arms: Vec<MsgMatchArm>
} 

impl Parse for ActorReceive {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let _: Token![match] = input.parse()?;
        let msg_name: Ident = input.parse()?;
        let _: Token![for] = input.parse()?;
        parse_ident_value(input, "Actor")?;
        let _: Token![<] = input.parse()?;
        let state_type: Path = input.parse()?;
        let _: Token![,] = input.parse()?;
        let msg_type: Path = input.parse()?;
        let _: Token![>] = input.parse()?;

        let where_clause: Option<WhereClause> = input.parse()?;

        let _: Token![as] = input.parse()?;

        let match_arms = parse_match_arms(input)?;

        Ok( ActorReceive { msg_name, msg_type, state_type, where_clause, match_arms } )
    }
}

fn collect_typevars<'a> (where_clause: &'a WhereClause) -> Vec<&'a Path> {
    let mut typevars = Vec::new();

    for where_predicate in &where_clause.predicates {
        if let WherePredicate::Type(predicate_type) = where_predicate {
            if let Type::Path(ref type_path)  = predicate_type.bounded_ty {
                typevars.push( &type_path.path)
            }
        }
    }

    typevars
}

/* #endregion actor receive definition */

/* #region match arm macros  *****************************************************/

/// statement (block) wrapper macro to be used in match arm expressions that makes sure we return 
/// [`ReceiveAction::Continue`] from this match arm 
/// 
/// Example:
/// ```
///     match_actor_msg_type! { msg: MyActorMsg as 
///         A => cont! { println!("actor received an A = {:?}", msg) }
///         ...
/// ```
/// This is expanded into:
/// ```
///     match msg {
///         A(msg) => { {println!("actor received an A = {:?}", msg)}; ReceiveAction::Continue }
///         ...
/// ```
#[proc_macro]
pub fn cont (ts: TokenStream)->TokenStream {
    expand_msg_match_action( ts, quote! { ReceiveAction::Continue })
}

/// statement (block) wrapper macro to be used in match arm expressions that makes sure we return 
/// [`odin_actor::ReceiveAction::Stop`] from this match arm. See [`cont`] for details.
#[proc_macro]
pub fn stop (ts: TokenStream)->TokenStream {
    expand_msg_match_action( ts, quote! { ReceiveAction::Stop })
}

/// statement (block) wrapper macro to be used in match arm expressions that makes sure we return 
/// [`odin_actor::ReceiveAction::Stop`] from this match arm. See [`cont`] for details.
#[proc_macro]
pub fn term (ts: TokenStream)->TokenStream {
    expand_msg_match_action( ts, quote! { ReceiveAction::RequestTermination })
}

/* #endregion match arm macros  */

/* #region spawn_actor ***********************************************************/

/// macro to spawn an actor for a generic actor state S
/// arguments: `(actor_system: &mut ActorSystem, actor_name: &str, actor_state: S [, channel_bounds: usize])`
/// 
/// use like so:
/// ```
/// let hserver = spawn_actor!( actor_system, "server", SpaServer::new(...), 64)?;
/// ```
#[proc_macro]
pub fn spawn_actor (item: TokenStream)->TokenStream {
    let SpawnActor { spawner, aname_expr, astate_expr, channel_bounds } = match syn::parse(item) {
        Ok(actor_receive) => actor_receive,
        Err(e) => panic!( "expected \"spawn_actor!( «actorSystem», «actorName», «actorState» [,«channelBounds»])\", got {:?}", e)
    };
    let cbounds = if let Some(channel_bounds) = channel_bounds { quote!{#channel_bounds} } else { quote!{ DEFAULT_CHANNEL_BOUNDS} };
    
    let new_item: TokenStream = quote! { 
        #spawner.spawn_actor( #spawner.new_actor( #aname_expr, #astate_expr, #cbounds)) 
    }.into();
    //println!("-----\n{}\n-----", new_item.to_string());

    new_item
}

struct SpawnActor {
    spawner: Expr,
    aname_expr: Expr,
    astate_expr: Expr,
    channel_bounds: Option<Expr>
}
impl Parse for SpawnActor {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let spawner: Expr = input.parse()?;
        let _: Token![,] = input.parse()?;
        let aname_expr: Expr = input.parse()?;
        let _: Token![,] = input.parse()?;
        let astate_expr: Expr = input.parse()?;

        let lookahead = input.lookahead1();
        let channel_bounds = if lookahead.peek( Token![,]) {
            let _: Token![,] = input.parse()?;
            let bounds_expr: Expr = input.parse()?;
            Some(bounds_expr)
        } else {
            None
        };

        Ok( SpawnActor { spawner, aname_expr, astate_expr, channel_bounds } )
    }
}

#[proc_macro]
pub fn spawn_dyn_actor (item: TokenStream)->TokenStream {
    let SpawnActor { spawner, aname_expr, astate_expr, channel_bounds } = match syn::parse(item) {
        Ok(actor_receive) => actor_receive,
        Err(e) => panic!( "expected \"spawn_dyn_actor!( «actorHandle», «actorName», «actorState» [,«channelBounds»])\", got {:?}", e)
    };

    let cbounds = if let Some(channel_bounds) = channel_bounds { quote!{#channel_bounds} } else { quote!{ DEFAULT_CHANNEL_BOUNDS} };

    let new_item: TokenStream = quote! {
        #spawner.hsys().spawn_actor( #spawner.new_actor( #aname_expr, #astate_expr, #cbounds))
    }.into();
    //println!("-----\n{}\n-----", new_item.to_string());

    new_item
}

#[proc_macro]
pub fn spawn_pre_actor (item: TokenStream)->TokenStream {
    let SpawnPreActor { spawner, h_pre_expr, astate_expr } = match syn::parse(item) {
        Ok(actor_receive) => actor_receive,
        Err(e) => panic!( "expected \"spawn_pre_actor!( «actorSystem», «actorName», «actorState»)\", got {:?}", e)
    };

    let new_item: TokenStream = quote! {
        #spawner.spawn_actor( #spawner.new_pre_actor( #h_pre_expr, #astate_expr))
    }.into();
    //println!("-----\n{}\n-----", new_item.to_string());

    new_item
}

struct SpawnPreActor {
    spawner: Expr,
    h_pre_expr: Expr,
    astate_expr: Expr,
}
impl Parse for SpawnPreActor {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let spawner: Expr = input.parse()?;
        let _: Token![,] = input.parse()?;
        let h_pre_expr: Expr = input.parse()?;
        let _: Token![,] = input.parse()?;
        let astate_expr: Expr = input.parse()?;

        Ok( SpawnPreActor { spawner, h_pre_expr, astate_expr})
    }
}

/* #endregion spawn_actor */

/* #region fnmut *****************************************************************/

// [([mut] id = expr {, ...}) =>] [| id [: type] {, ...} |] expr )

/// syntactic sugar macro that translates
/// ```
///   fn_mut!( (mut var1 = foo(), var2 = bar) => |a,b| {
///      do_something_with_captures_and_args( &var1, &var2, a, b);
///      var2 = compute_new_var2(..); // possible mutate mut captures
///   });
/// ```
/// into 
/// ```
/// {
///   let mut var1 = foo();
///   let var2 = bar;
///   move |a,b| { ... }   
/// }
/// ```
#[proc_macro]
pub fn fn_mut (item: TokenStream)->TokenStream {
    let FnMutSpec{ var_bindings, args, body} = match syn::parse(item) {
        Ok(spec) => spec,
        Err(e) => panic!( "expected fn_mut!( [([mut] «id» = «expr» {{, ...}}) =>] [| «id» [: «type»] {{, ...}} |] «expr» ) , error: {:?}", e)
    };

    let new_item: TokenStream =quote! {
        {
            #( let #var_bindings ;)*
            move | #( #args ),* | { #body }
        }
    }.into();
    //println!("-----\n{}\n-----", new_item.to_string());

    new_item
}

struct FnMutSpec {
    var_bindings: Vec<VarAssign>,
    args: Vec<OptTypedVar>,
    body: Expr
}

impl Parse for FnMutSpec {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let mut var_bindings: Vec<VarAssign> = Vec::new();
        let mut lookahead = input.lookahead1();
        if lookahead.peek( Paren) {
            let var_input;
            let _var_paren: Paren = parenthesized!( var_input in input);
            let mut dvars: Punctuated<VarAssign,Comma> = Punctuated::parse_separated_nonempty(&var_input)?;
            var_bindings = dvars.into_iter().collect::<Vec<_>>().into();

            let _: Token![=>] = input.parse()?;
            lookahead = input.lookahead1();
        }

        let mut args: Vec<OptTypedVar> = Vec::new();
        if lookahead.peek( Token![|]) {
            let _: Token![|] = input.parse()?;
            loop {
                args.push( input.parse()?);
                lookahead = input.lookahead1();
                if !lookahead.peek(Token![,]) { break; } else { input.parse::<Token![,]>()?; }
            }
            input.parse::<Token![|]>()?;
        }

        let body: Expr = input.parse()?;

        Ok( FnMutSpec{var_bindings, args, body} )
    }
}

/* #endregion fnmut */

/* #region public_struct *********************************************************/

/// syntactic sugar to make visibility of a struct and all its fields public
/// use like so:
/// ```
/// #[public_struct]
/// struct Foo {
///   field: X,
///   ....
/// }
/// ```
#[proc_macro_attribute]
pub fn public_struct (attr: TokenStream, tokens: TokenStream) -> TokenStream {
    parse_macro_input!(attr as Nothing);
    let mut item_struct = parse_macro_input!(tokens as ItemStruct);

    item_struct.vis = match &item_struct.vis {
        syn::Visibility::Public(p) => syn::Visibility::Public(*p),
        syn::Visibility::Restricted(res) => syn::Visibility::Restricted(res.clone()),
        syn::Visibility::Inherited => parse_quote!(pub),
    };

    for field in &mut item_struct.fields {
        field.vis = match &field.vis {
            syn::Visibility::Public(p) => syn::Visibility::Public(*p),
            syn::Visibility::Restricted(res) => syn::Visibility::Restricted(res.clone()),
            syn::Visibility::Inherited => parse_quote!(pub),
        };
    }
    item_struct.to_token_stream().into()
}

/* #endregion public_struct */

/* #region support funcs *********************************************************/

// just a simple "var_name: var_type" fragment that can be used in various places where the full
// complexity of respective syn types (pattern matching, visibility etc) is unwanted
struct TypedVar {
    var_name: Ident,
    colon_token: Colon,
    var_type: Type
}

impl Parse for TypedVar {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let var_name: Ident = input.parse()?;
        let colon_token: Colon = input.parse::<Token![:]>()?;
        let var_type: Type = input.parse()?;
        Ok(TypedVar{var_name,colon_token,var_type})
    }
}

impl ToTokens for TypedVar {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        self.var_name.to_tokens(tokens);
        self.colon_token.to_tokens(tokens);
        self.var_type.to_tokens(tokens);
    }
}

// var whose type can be inferred
struct OptTypedVar {
    var_name: Ident,
    colon_token: Option<Colon>,
    var_type: Option<Type>
}

impl Parse for OptTypedVar {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let var_name: Ident = input.parse()?;
        let mut colon_token: Option<Colon> = None;
        let mut var_type: Option<Type> = None;

        let lookahead = input.lookahead1();
        if lookahead.peek(Token![:]) {
            colon_token = Some(input.parse::<Colon>()?);
            var_type = Some(input.parse()?);
        }
        Ok(OptTypedVar{var_name,colon_token,var_type})
    }
}

impl ToTokens for OptTypedVar {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        self.var_name.to_tokens(tokens);
        self.colon_token.to_tokens(tokens);
        self.var_type.to_tokens(tokens);
    }
}


struct VarAssign {
    maybe_mut_token: Option<Mut>,
    var_name: Ident,
    assign_token: Token![=],
    init_expr: Expr
}

impl Parse for VarAssign {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let lookahead = input.lookahead1();
        let maybe_mut_token = if lookahead.peek(Mut) { Some(input.parse::<Mut>()?) } else { None };
        let var_name: Ident = input.parse()?;
        let assign_token: Token![=] = input.parse()?;
        let init_expr: Expr = input.parse()?;

        Ok(VarAssign{ maybe_mut_token, var_name, assign_token, init_expr })
    }
}

impl ToTokens for VarAssign {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        if let Some(t) = self.maybe_mut_token { t.to_tokens(tokens) }
        self.var_name.to_tokens(tokens);
        self.assign_token.to_tokens(tokens);
        self.init_expr.to_tokens(tokens);
    }
}

fn get_type_base (t: &Type)->TokenStream2 {
    match t {
        Type::Reference(type_reference) => { let elem = &type_reference.elem; quote!{ #elem } }
        _ => quote!{ #t } // TODO - there are probably Type variants we have to reject
    }
}

const N_SYS_MSGS: usize = 7;
const SYS_MSGS: [&'static str; N_SYS_MSGS] = [
    "_Start_", "_Ping_", "_Timer_", "_Exec_", "_Pause_", "_Resume_", "_Terminate_"
];

fn get_sys_msg_idents()->[Ident;N_SYS_MSGS] {
    SYS_MSGS.map( |i| { Ident::new(i, Span::call_site())})
}

fn get_sys_msg_types()->[Path;N_SYS_MSGS] {
    SYS_MSGS.map( |i| { 
        let ident = Ident::new(i, Span::call_site());
        let mut segments = Punctuated::new();
        segments.push( PathSegment { ident, arguments: syn::PathArguments::None } );
        Path { leading_colon: None, segments}
    })
}

fn expand_msg_match_action (ts: TokenStream, ret_val: TokenStream2)->TokenStream {
    let body = TokenStream2::from(ts); // we need a TokenStream2 to get a ToToken impl
    let new_item: TokenStream =quote! {
        { { #body }; #ret_val }
    }.into();

    new_item
}

fn variant_spec_to_string (var_spec: &VariantSpec)->String {
    match var_spec {
        VariantSpec::Type(path) => {
            let ts: TokenStream = quote! { #path }.into(); // ..a bit lazy
            ts.to_string()
        }
        VariantSpec::Wildcard => {
            "_".to_string()
        }
    }
}

fn path_to_string (path: &Path)->String {
    let ts: TokenStream = quote! { #path }.into(); // ..a bit lazy
    ts.to_string()
}

/// turn a type (Path) into a valid Ident string
/// Note this does not need to be reversible since our macros only use valid type strings as
/// input and the mangled name is never seen be the user. 
/// The mapping only needs to be locally unique, i.e. it should not collide with a user-provided
/// type. For that reason the mapping should not use any commonly used chars but still produce
/// reasonably readable Debug output. 
/// symmetric candidates:
///     Ͼ \u{03fe} , Ͽ \u{03ff}
///     ᄼ \u{113c} , ᄾ \u{113e}
///     ᐊ \u{140a} , ᐅ \u{1405} 
///     ᐸ \u{1438} , ᐳ \u{1433}
///     ᑕ \u{1455} , ᑐ \u{1450}
///     ʕ \u{0295} , ʔ \u{0294}
///     ʃ \u{0283} , ʅ \u{0285}
///     𐅁 \u{10141} ,  𐅀 \u{10140}
///     𛰙 \u{1bc19} ,  𛰚 \u{1bc1a}
///     𑄼 \u{1113c}
///     𖫫 \u{16aeb}
///     
/// Candidates from https://util.unicode.org/UnicodeJsps/list-unicodeset.jsp?a=[:XID_Continue=Yes:]
fn mangle (s: &str)->String {
    let mut r = String::with_capacity(s.len());
    let mut lifetime = false;
    for c in s.chars() {
        match c {
            ':' => r.push('\u{02d0}'), // 'ː'
            '<' => r.push('\u{1438}'), // 'ᐸ' 
            '>' => r.push('\u{1433}'), // 'ᐳ'
            ',' => r.push('\u{02ce}'), // 'ˎ'
            '(' => r.push('\u{1bc19}'), // '𛰙'
            ')' => r.push('\u{1bc1a}'), // '𛰚'
            '&' => r.push('\u{1113c}'), // '𑄼'
            '\'' => { lifetime = true; r.push('\u{02bd}') }, // 'ʽ'
            ' ' => if lifetime { lifetime = false; r.push('\u{02d1}') }, // 'ˑ'
            _ => r.push(c)
        }
    }
    r
}

// this can be used for pseudo keywords
fn parse_ident_value (input: ParseStream<'_>, expected: &str)->syn::Result<()> {
    let ident: Ident = input.parse()?;
    if ident != expected {
        Err( syn::Error::new(ident.span(), format!("expected `{}`", expected)))
    } else {
        Ok(())
    }
}

fn parse_visibility (input: ParseStream) -> Visibility {
    let lookahead = input.lookahead1();
    if lookahead.peek(Token![pub]) {
        input.parse::<Visibility>().unwrap()
    } else {
        Visibility::Inherited
    }
} 

/* #endregion support funcs */
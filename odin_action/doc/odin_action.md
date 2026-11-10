# odin_action

The `odin_action` crate provides several variants of **action** types together with macros to define and instantiate ad
hoc actions. The generic **action** construct represents application specific objects that encapsulate async
computations, to be executed by an *action owner* that can invoke such computations with its own data (e.g. sending
messages in actor systems that are built from its data).

The primary purpose of actions is to build re-usable action owners that do not have to be aware of in which
application context they are used. All the owner has to know is when to execute an action and what of its own data
it should provide as an argument.

In a synchronous world this is often described as a "callback".

The basis for this are "Action" traits with a single `async fn execute(&self,data..)->Result<()>` method. Instances of
these traits are normally created where we assemble an application (e.g. in `main()`), i.e. where we know all the
relevant interaction types. They are then passed either as generic type constructor arguments or later-on (at runtime)
as trait objects to their owners, to be invoked either on-demand or when the owner state changes.

Technically, actions represent a special case of async closures in which capture is done by either `Copy`
or `Clone`. Reference capture is not useful here since actions are executed within another task, without any
lifetime relationship to the context in which the actions were created.
 
We support the following variants:
 
- [`DataAction<T>`] trait and [`data_action`] macro
- [`DataMutAction<T>`] trait and [`data_mut_action`] macro (for actions that mutate their own state)
- [`BiDataAction<T,A>`] trait and [`bi_data_action`] macro
- [`DynDataAction<T>`] type and [`dyn_data_action`] macro

Owner data is passed into the trait's `execute(..)` function either as a moved value (`DataAction<Foo>`) or as a
reference. Since actions are executed by owners that pass references to their own data (e.g. actor state) the
respective action types have to accept references of arbitrary lifetimes, i.e. they are constrained by higher ranked
trait bounds such as `for<'a> DataAction<&'a Foo>`. The macros automatically create such actions if the data argument
is a reference:

```rust
let action = data_action!( let v: u64 = v => |data: &Foo| { ... }); // impl<'a> DataAction<&'a Foo> for ..
..
struct MyActor<A> where A: for<'a> DataAction<&'a Foo> { .. }        // or A: DataRefAction<Foo>
```

For brevity (and backwards compatibility) there are aliases for these reference actions:

| alias | equivalent to |
|---|---|
| [`DataRefAction<T>`] trait, [`dataref_action`] macro | `for<'a> DataAction<&'a T>`, `data_action!(..=> \|data: &T\| ..)` |
| [`DataRefMutAction<T>`] trait, [`dataref_mut_action`] macro | `for<'a> DataMutAction<&'a T>`, `data_mut_action!(..=> \|data: &T\| ..)` |
| [`BiDataRefAction<T,A>`] trait, [`bi_dataref_action`] macro | `for<'a> BiDataAction<&'a T,A>`, `bi_data_action!(..=> \|data: &T, bidata: A\| ..)` |
| [`DynDataRefAction<T>`] type, [`dyn_dataref_action`] macro | `Box<dyn for<'a> DynDataActionTrait<&'a T>>`, `dyn_data_action!(..=> \|data: &T\| ..)` |

The alias traits are automatically implemented for all respective actions - they cannot (and do not need to) be
implemented explicitly. The same holds for other data types with references (e.g. `for<'a> DataAction<Vec<&'a Foo>>`).

The empty [`NoAction`] (created by `no_data_action()`, `no_dataref_action()` etc.) implements all non-dyn action
traits for all data types and can be used where owners have optional actions.
 
The `Bi..Action<T,B>` traits have `execute(..)` functions that take two arguments (of potentially different types). This is
helpful in a context where the action body requires both owner state (`T`) and information that was passed to the 
owner (`B`) in the request that triggers the action execution and can avoid the runtime overhead of async action trait
objects (requiring `Pin<Box<dyn Future ..>>` execute return values). The limitation of bi-actions is that both
action owner and requester have to know the bi_data type (`B`), which therefore tends to be unspecific (e.g. `String`).
This in turn makes bi-actions more susceptible to mis-interpretation and therefore the action owner should only use
`B` as a pass-through argument and not generating it (which would require the owner knows what potential requesters
expect semantically).

`Dyn..Action` types (which represent trait objects) are used in two different contexts:

- to execute actions that were received as function arguments (e.g. through async messages)
- to store such actions in homogenous `Dyn..ActionList` containers for later execution

The `Dyn..ActionList` containers use an additional `ignore_err: bool` argument in their `execute(..)` methods
that specifies if the execution should shortcut upon encountering error results when executing its stored actions
or if return values of stored actions should be ignored.

```rust
struct MyActor { ...
    data: MyData, 
    actions: DynDataRefActionList<MyData>
}
...
impl MyActor {
    async fn exec (&self, da: DynDataRefAction<MyData>) {
        da.execute(&self.data).await;
    }

    fn store (&mut self, da: DynDataRefAction<MyData> ) { 
        .. self actions.push( da) ..
    }
    ... self.actions.execute(&self.data, ignore_err).await ...
}
```

Note that `Dyn..Action` instances do have runtime overhead (allocation) per `execute(..)` call.

Since actions are typically one-of-a-kind types we provide macros for all the above variants that both define the type
and return an instance of this type. Those macros all follow the same pattern:

```rust
//--- system construction site:
let v1: String = ...
let v2: u64 = ...
let action = data_action!{ 
    let v1: String = v1.clone(), 
    let v2: u64 = v2 => 
    |data: Foo| {
        println!("action executed with arg {:?} and captures v1={}, v2={}", data, v1, v2);
    Ok(())
    }
};
let actor = MyActor::new(..action..);
...
//--- generic MyActor implementation:
struct MyActor<A> where A: DataAction<Foo> { ... action: A ... }
impl<A> MyActor<A> where A: DataAction<Foo> {
  ... let data = Foo{..}
  ... self.action.execute(data).await ...
}
```
the example above expands into a block with three different parts: capture struct definition, action trait impl and capture struct instantiation

```rust
{
    struct SomeDataAction { v1: String, v2: u64 }

    impl DataAction<Foo> for SomeDataAction {
         async fn execute (&self, data: Foo)->std::result::Result<(),OdinActionFailure> {
             let v1 = &self.v1; let v2 = &self.v2;
             println!(...);
             Ok(())
         }
    }

    SomeDataAction{ v1: v1.clone(), v2 }
}
```

The action bodies are expressions that have to return a `Result<(),OdinActionFailure>`, which is the common error
type of all actions so that we can store actions with the same data type in homogenous containers. Since
[`OdinActionFailure`] can be created from anything that implements [`std::fmt::Display`] we can use the `?` operator
within action bodies without explicit error mapping:

```rust
fn compute_some_result(...)->Result<Foo,SomeError> {...}
...
data_action!( ... => |data: MyData| {
    ...
    let foo = compute_some_result(...)?; // SomeError is converted into OdinActionFailure
    ...
    Ok(())
})
```

Crates that want to map `OdinActionFailure` into their own error types can use the [`map_action_failure`] macro.

## Why Actions Are Not Async Closures

Since Rust 1.85 there are native async closures (`async |data: Foo| {..}`) and respective `std::ops::AsyncFn` /
`AsyncFnMut` traits. Conceptually these would be a natural replacement for our action traits - they capture their
environment, support reference arguments with arbitrary lifetimes and can have any number of arguments (which would
make `Bi..Action` variants obsolete). We still use our own action traits for the following reasons:

1. **`Send` futures in generic owners** - the primary use of actions is within actors (see
   [odin_actor](../odin_actor/odin_actor.md)), which execute in spawned tasks and therefore require that all futures
   they await are `Send`. Since action owners are generic over their action types (that is the whole point of actions)
   this has to be provable for any action type parameter. `DataAction` guarantees this by declaring the return type of
   `execute(..)` as `impl Future<..> + Send`. For async closures there is no way to express this on stable Rust yet -
   it requires either [return type notation](https://github.com/rust-lang/rust/issues/109417)
   (`A: AsyncFn(&Foo)->Result<..>, A::call(..): Send`) or nameable closure future types (`AsyncFnMut::CallRefFuture`,
   which is part of the unstable `async_fn_traits` feature). Without such a bound a generic owner fails to compile with
   "future cannot be sent between threads safely". Async closures only work if the owner knows the concrete closure type.
2. **trait objects** - `AsyncFn` traits are not dyn-compatible, i.e. we would need our own [`DynDataActionTrait`]
   anyways to send actions in messages or to store them in homogenous containers ([`DynDataActionList`]).
3. **debugging** - closures do not implement `Debug`, which our action traits require so that owners (e.g. actor
   states) can be logged and inspected.
4. **empty actions** - [`NoAction`] (and the `is_empty()` method) would have to be replaced by `Option<..>` values in
   all owners.

To keep a future transition to native async closures simple **action instances should always be created through the
macros of this crate** (`data_action!`, `dyn_data_action!` etc.) and not by explicit trait implementations. The macro
syntax (`let «var»: «type» = «expr» .. => |«data»: «type»| «expr»`) already corresponds to a closure with explicit
(clone) captures. Once the `Send` bound problem is solved in stable Rust these macros can expand into async closures and
the action traits (resp. their [`DataRefAction`] style aliases) into respective `AsyncFn` bounds without changing
the code that creates or executes actions. Only the dyn actions are likely to remain as they are.

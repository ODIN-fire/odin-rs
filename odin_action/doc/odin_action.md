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
 
- [`DataAction<T>`] trait and ['data_action`] macro
- [`DataRefAction<T>`] trait and ['dataref_action`] macro
- [`BiDataAction<T,A>`] trait and [`bi_data_action`] macro
- [`BiDataRefAction<T,A>`] trait and [`bi_dataref_action`] macro
- [`DynDataAction<T>`] type and ['dyn_data_action`] macro
- [`DynDataRefAction<T>`] type and ['dyn_dataref_action`] macro

The difference between `..DataAction` and `..DataRefAction` is how the owner data is passed into the trait's
`execute(..)` function: as a moved value (`execute(&self,data:T)`) or as a reference (`execute(&self,data:&T)`).
 
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
    actions: DynDataActionList<MyData>
}
...
impl MyActor {
    async fn exec (&self, da: DynDataAction<MyData>) { 
        da.execute(&self.data).await;
    }

    fn store (&mut self, da: DynDataAction<MyData> ) { 
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
         async fn execute (&self, data: Foo)->std::result::Result<(),OdinActionError> {
             let v1 = &self.v1; let v2 = &self.v2;
             println!(...);
             Ok(())
         }
    }

    SomeDataAction{ v1: v1.clone(), v2 }
}
```

The action bodies are expressions that have to return a `Result<(),OdinActionError>` so that we can coerce errors in crates using
`odin_action`. This means that we can use the `?` operator to shortcut within action bodies, but we have to map respective results
by means of our `map_action_err()` function and make sure to use `action_ok()` instead of explicit `Ok(())` (to tell the compiler
what `Result<T,E>` it refers to):
 
```rust
fn compute_some_result(...)->Result<(),SomeError> {...}
...
data_action!( ... => |data: MyData| {
    ...
    map_action_err( compute_some_result(...) )?
    ...
    action_ok()
})
```
 
For actions that end in a result no mapping is required (`map_action_err(..)` is automatically added by the macro expansion):

```rust
data_action!( ... => |data: MyData| {
    ...
    compute_some_result(...)
})
```

[`OdinActionError`] instances can be created from anything that implements [`ToString`]`

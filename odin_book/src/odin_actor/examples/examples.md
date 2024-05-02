# Examples

The `odin_actor/examples` directory contains a set of runnable example applications that each introduce and demonstrate a single
`odin_actor` feature. It is recommended to go through examples in the following sequence: 

- [`hello_world`](odin_actor/examples/hello_world.md) : the basics (actorsystem, actor and sending messages) 
- [`sys_msgs`](odin_actor/examples/sys_msgs.md) : using system messages and timers
- [`spawn`](odin_actor/examples/spawn.md) : spawning one-shot async tasks from within actors
- [`spawn_blocking`](odin_actor/examples/spawn_blocking.md) : spawn blocking tasks (running in threads) from within actors
- [`exec`](odin_actor/examples/exec.md) : using the generic `exec(..)` to execute closures within actor tasks
- [`jobs`](odin_actor/examples/jobs.md) : scheduling generic jobs with the actor system global `JobScheduler`
- [`producer_consumer`](odin_actor/examples/producer_consumer.md) : point-to-point actor communication with `MsgReceiver`
- [`pub_sub`](odin_actor/examples/pub_sub.md) : publish/subscribe communication with `MsgSubscriptions`
- [`ping_pong`](odin_actor/examples/pin_pong.md) : managing cyclic actor dependencies with `PreActorHandle`
- [`query`](odin_actor/examples/query.md) : using `Query<Q,A>` to send a message and wait for an answer 
- [`dyn_actor`](odin_actor/examples/dyn_actor.md) : dynamically create actors from within actors
- [`actions`](odin_actor/examples/actions.md) : statically configure actor interaction with `DataAction`
- [`dyn_actions`](odin_actor/examples/dyn_actions.md) : dynamically configure actor interaction with `DynDataAction`
- [`retry`](odin_actor/examples/retry.md) : handling back-pressure with `retry_send_msg(..)`
- [`requests`](odin_actor/examples/requests.md) : sequential processing of requests in background task
- [`actor_config`](odin_actor/examples/actor_config.md) : configuring actors with the `config_for!(..)` macro
- [`heartbeat`](odin_actor/examples/heartbeat.md) : monitoring actor systems with heartbeat messages

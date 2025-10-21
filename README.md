# Pure Rust wayland protocol implementation

This is my from-scratch implementation of the wayland protocol, some surrounding
tooling and random things I am writing with it, which might at some point end up
with me *actually* writing a full wayland compositor...

The name is currently very much undecided on.
If you have some naming ideas [open to suggestions](https://github.com/titaniumtraveler/ecs-compositor/discussions/1).

Currently I am using the name `ecs-compositor` as placeholder, based on an old
idea of implementing a compositor using an Entity Component System, but it is
definitely not a name I'm happy with.

## Current Status

Everything in here should be treated as very much experimental and I will break
code and APIs just because I feel like so.

Though as basic courtesy I will try to make every commit at least `cargo check`/`cargo clippy` run,
ideally without warnings.

## Subprojects

In `/crates/*` I have a few library-like crates. Notable examples are:

- [`ecs-compositor-core`](./crates/core/README.md)
  which provides the core wayland primitives and abstractions

- [`ecs-compositor-codegen`](./crates/codegen/) <!-- TODO: README -->
  which provides the functions to generate wayland bindings as part of `build.rs`.
  See [`examples/wayland-raw`](./examples/wayland-raw/) for how to set that up. <!-- TODO: README -->

In `/examples/*` I have (as the name suggests) examples, which are both used to
explore new APIs as well as keeping them as education material to demonstrate
some API uses.

- [`examples/wayland-raw`](./examples/wayland-raw/)
  demonstrates how one can *manually* send and receive wayland messages
- [`examples/wayland-tokio`](./examples/wayland-tokio/)
  which contains an implementation of an asynchronous wayland client.

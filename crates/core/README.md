# `ecs-compositor-core`

Library containing the core wayland primitives and basic abstractions.

## Core Primitives

See the [wayland wire-format description](https://wayland.freedesktop.org/docs/html/ch04.html#sect-Protocol-Wire-Format) for context.

`int`, `uint`, `fixed`
are just wrappers around `i32`/`u32`s as per specification

`enumeration`
Wayland enumerations are represented as `#[repr(u32)]` enums that implement the `enumeration` trait.

`new_id<_>`, `object<_>`
are wrappers around an object id (a `NonZero<u32>`) and carry the type
information of the `Interface` they are belonging to.
(With `()` being used as a way to cast the interface away)

In cases where `object<_>` is optional `Option<object<_>>` should be used.
Wayland doesn't allow optional `new_id<_>`s.

`new_id_dyn`
is used for the dynamic interface mechanism used prominently in `wl_registry.bind(name: uint, id: new_id)`
to specify requested interface version.

Generally `new_id` arguments have a interface statically defined by the `xml`
file defining the API, but when it is not specified, instead a triple of `{ name: string, version: uint, id: new_id }` is used.

`array<'data>`, `string<'data>`
are implemented as a pointer to a buffer + the length of that buffer.
Per default that pointer is directly pointing at the buffer the value was read
from.

If the pointer is `null`, that **does not** imply that the value is none, but
that it is already serialized: That means a user can write a dynamic string *directly*
to the output buffer and set only `string.len` and the write call of the message
will only write the header of the string and skip the written bytes, instead of
needing to use a temporary buffer + a `memcopy`.

**Note**: While `string<'data>` and `array<'data>` include a lifetime that
should be pointed at their buffer, that lifetime is *essentially* provided by
the user and is in no way validated.
Which means that constructing them incorrectly can quickly lead to undefined behavior!

Also note that the length doesn't point at the full buffer, as the wayland
protocol defines that the buffer should be padded to a 4 byte boundary.

So the *actual* length of the buffer an `array`/`string` occupies is `align::<4>(len)` instead of just `len`.
(`align::<4>(len)` being implemented as `(len + 4 - 1) & !(4 - 1)`) 

`fd` 
is a wrapper around an `i32` representing a file descriptor that was sent via
ancillary data and might or might not have the Close-on-Exec flag set depending
on how the `recvmsg` call that received it was configured.

The user has to take care to close it.

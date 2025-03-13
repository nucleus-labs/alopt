# alopt

`alopt` (Arc-Lock-Option) is a Rust crate providing efficient synchronization primitives that integrate
`Option` into their design.

## Features

- **`Worl<T>`**: A replacement for `RwLock<Option<T>>`, integrating optional storage directly into a
lockable structure.
- **`Wom<T>`**: A replacement for `Mutex<Option<T>>`, integrating optional storage directly into a
lockable structure.
- **Optimized for Conditional Access**: Efficiently handles cases where the inner value may or may not exist.

## Example Usage

`Worl<T>`:
```rust
use std::sync::Arc;
use std::thread;

use alopt::thread::Worl;

let worl = Arc::new(Worl::new(42));
let worl_clone = worl.clone();

let handle = thread::spawn(move || {
    let mut guard = worl_clone.write().unwrap();
    *guard = 100;
});

handle.join().unwrap();
assert_eq!(*worl.read().unwrap(), 100);
```

`Wom<T>`:
```rust
use std::sync::Arc;
use std::thread;

use alopt::thread::Wom;

let wom = Arc::new(Wom::new(0));
let wom_clone = wom.clone();

*wom.get_mut().unwrap() = 42;

let handle = thread::spawn(move || {
    let mut guard = wom_clone.lock().unwrap();
    *guard += 100;
});

handle.join().unwrap();
assert_eq!(*worl.read().unwrap(), 142);
```

## Why Use `alopt`?

- **Integrated `Option` Handling**: Eliminates the need for `RwLock<Option<T>>` or `Mutex<Option<T>>`,
reducing unnecessary layers of indirection.
- **Designed for Simplicity**: Provides a clean API for conditional value access while maintaining safe concurrency.
- **Optimized for Practical Use Cases**: Reduces boilerplate and improves ergonomics when dealing with
optional values in a multi-threaded context.

## Installation

Add `alopt` to your `Cargo.toml`:

```toml
[dependencies]
alopt = "0.1"
```

## License

`alopt` is licensed under the MIT License.

loopfisch
=========

An audio and MIDI loop machine written in Rust.

This is still in active development, but the basic functionality already works.

![build](https://github.com/Windfisch/loopfisch/workflows/build/badge.svg)
![test suite](https://github.com/Windfisch/loopfisch/workflows/test%20suite/badge.svg)
[![codecov](https://codecov.io/gh/Windfisch/loopfisch/branch/master/graph/badge.svg?token=TTT5P17K18)](https://codecov.io/gh/Windfisch/loopfisch)

Features
--------

- [JACK Audio](https://jackaudio.org) output
- Audio and MIDI support
- Simultaneous recording of both Audio and MIDI at a time
- As seamless as possible switching between both
- Multiple output chains (implemented by having multiple JACK ports)
- Browser-based user interface
- Fully (PC-)keyboard-controllable (_not yet_)
- MIDI clock master
- MIDI transport slave (_not yet_)
- Unit tests (_partially_)

Build instructions
------------------

Currently, you need to use the *unstable* Rust toolchain, version 1.50 or later. 
You can add a per-directory override using

```
rustup override set nightly
rustup update # might or might not be needed
```

You might need to update rust, for more information, you can refer to
https://rocket.rs/v0.4/guide/getting-started/.

Usage
-----

Start a jack server, then launch the engine using `cargo run`. You should now be able to make REST requests
like `curl localhost:8000 /api/synths`.

In order to be able to use the GUI, change to the [web](web) directory and run `yarn serve`. (You might need to `yarn install` before). Then access [http://localhost:8080](http://localhost:8080) in your web browser.

Test suite
----------

Run the tests with `cargo test`.

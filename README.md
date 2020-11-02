loopfisch
=========

My attempts in writing a audio and MIDI loop machine in Rust.

This is in no way a ready or usable piece of software. In fact, it is little
more than a evolving proof-of-concept right now, requiring some code cleanup
in the long run.

However, we can already talk about where we want to go:

Features
--------

- [JACK Audio](https://jackaudio.org) output
- Audio and MIDI support
- Simultaneous recording of both Audio and MIDI at a time
- As seamless as possible switching between both
- Multiple output chains (implemented by having multiple JACK ports)
- Browser-based user interface
- Fully (PC-)keyboard-controllable (_not yet_)
- MIDI clock master, MIDI transport slave (_not yet_)

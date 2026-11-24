# deslop-please

[![slop](https://img.shields.io/endpoint?url=https%3A%2F%2Fraw.githubusercontent.com%2FTommasoAmici%2Fdeslop-please%2Fbadges%2Fslop.json)](https://github.com/TommasoAmici/deslop-please/actions/workflows/ci.yml?query=branch%3Amain)
[![test](https://github.com/TommasoAmici/deslop-please/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/TommasoAmici/deslop-please/actions/workflows/ci.yml?query=branch%3Amain)

An opinionated linter to remove sloppy code. It follows [*A Philosophy of Software Design*](https://www.amazon.com/Philosophy-Software-Design-John-Ousterhout/dp/1732102201): 
**deep** modules, **narrow** interfaces.

This fork adds Swift package support through `scip-swift`. The Rust workspace path and its lints are unchanged.


```bash
cargo install deslop-please
```

For Python, TypeScript and JavaScript, see the [port](https://github.com/Bor4brn/deslop-please).

Requires `rust-analyzer` (e.g., `rustup component add rust-analyzer`).

## Swift

Run it inside a Swift package:

```bash
deslop-please
```

Swift support uses [`scip-swift`](https://github.com/jarvis-intelligence/scip-swift), a macOS 14+ executable that
turns SwiftPM's IndexStoreDB records into a SCIP index. Install its macOS binary from the releases page and put
`scip-swift` on `PATH`. The linter runs `swift package describe` to find the targets, then lets `scip-swift` build and
index the package. The index is cached under `.build/deslop-please/swift/` and rebuilt when a source, the manifest,
Swift or `scip-swift` changes.

To index separately, or to use an index made on another machine:

```bash
scip-swift /path/to/package
cd /path/to/package
deslop-please --scip index.scip
```

`--scip` still reads the package around the current directory for its source files and target layout. Cargo's
`--target` and `--features` flags do not apply to Swift and are rejected there.

A SwiftPM target is the package in the report. A directory directly containing source files is a module, because Swift
has no per-file module boundary but a concrete directory is still the unit a reader lists and a feature is kept in.
Directories that only group other modules are namespaces, not interfaces of their own. A `module-cycle` therefore
means two concrete directories in one target use each other's types, not that two Swift modules import each other.
Test targets are test code and do not form cycles with production code.

The same six lints run: `field-leak` follows a stored or computed property through its getter and setter;
`shallow-module` counts methods and free functions but not synthesized accessors; `tangle` finds types that know each
other in a circle; and `wide-directory` and `long-file` use every source SwiftPM lists, including files with no symbol
occurrences. Symbols whose only identity is a raw compiler USR and that have no resolved display name are left out,
because the model cannot safely say which type or member they belong to. A symbol also needs a definition range that
spells its name in the source. Compiler-generated members such as Observation backing fields are left out; synthesized
accessors remain when their source property is present.

```bash
> deslop-please

error[field-leak]: `RunQueue::items` is known outside `RunQueue`: 4 places in 2 modules
  --> src/sched/policy.rs:7:1
  = note: src/sched/policy.rs:7:1: `runqueue::sched::policy::admit`
  = note: src/sched/policy.rs:12:1: `runqueue::sched::policy::next`
  = note: src/sched/report.rs:4:1: `runqueue::sched::report::backlog`
  = note: src/sched/report.rs:8:1: `runqueue::sched::report::drained`
    runqueue::queue::ring::RunQueue
      items ◀━┳━ 2  runqueue::sched::policy
              ┗━ 2  runqueue::sched::report
      also known outside: head
      kept: limit

deslop-please: 65535 errors, well done!
```

### It's not my fault 

Sure, let's just mute them:

```bash
deslop-please --update-allow .deslop-please.allow  
deslop-please --allow .deslop-please.allow 
```

### Add to your `AGENTS.md`

```markdown
-  Run [`deslop-please`](https://github.com/deterministic-machines/deslop-please) to check for sloppy code structure.
-  Errors flagged by `deslop-please` must be fixed from first principles, don't do mechanical moves to hide them.
    - `module-cycle` & `tangle`: what is the indented layer of abstraction? Which module should be at which layer?
    - `field-leak` & `shallow-module`: Why we have these fields/functions at all? What are the patterns of use? Can they be hidden behind a narrow interface? Don't fix each leak one at a time, fix them holistically.
    - Draw a diagram to help user understand the before and proposed after structure.
```

## Why they are sloppy 

They are really just one single idea: **deep** modules, **narrow** interfaces.

Deep modules hide the attention for you and your agents, so you only need to care about the narrow interface.

### module-cycle

![module-cycle: before, modules kinds and socket use each other, though listen, Stream::new and BACKLOG_MOST only form a chain; after, BACKLOG_MOST moves into kinds and only socket uses kinds](docs/module-cycle.svg)


### tangle

![tangle: before, Tasks in kernel and Timers in its child module timers know each other, with no module cycle; after, tick in kernel calls both and Timers no longer knows Tasks](docs/tangle.svg)

### field-leak

![field-leak: before, functions in sched push to, index and bump the items and head of RunQueue; after, they call push, pop and backlog, and only RunQueue works on its fields](docs/field-leak.svg)

### shallow-module


![shallow-module: before, app calls 9 of the 15 functions of store; after, it calls new, load and summary, and the rest are hidden behind them](docs/shallow-module.svg)

## License

MIT

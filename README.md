# Advent of Code 2019 Intcode JIT

Your Intcode is running too slow? Use this x86 JIT compiler!

# Performance

The [included example](./fibonacci.intcode) calculates Fibonacci numbers mod signed 64 bit ints, run it like this.

```bash
cargo +nightly run --release -- fibonacci.intcode 1000
```

(Requires a Rust nightly build, install with `rustup install nightly`)

It's super-fast: Calculating the 100000000th Fibonacci number only takes 420 milliseconds on my laptop, that's 1.6 billion Intcode instructions per second!

My [Intcode interpreter](https://github.com/emmericp/advent-of-code-2019/blob/d2e79463161871e086b7ef34cd03623b149eea26/src/intcode.rs) written in Rust runs for 4.5 seconds on the same input, so this JIT is more than 10 times faster. 


# Limitations

Don't run this on your production starship computer, because:

* indirect jumps are NYI
* self-modifying code is not supported
* all instructions must be "aligned", i.e., you cannot jump into the middle of an instruction to interpret parameters as opcodes
* trailing data that looks like instructions is not handled, e.g., don't end your code with 1 data because the compiler will try to compile that as an `ADD` and complain about missing operands

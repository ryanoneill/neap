# Neap

A statically-typed, ML-inspired language for shell scripting.

Neap brings modern functional programming features to shell scripting, combining the expressiveness of ML-family languages with the practical needs of shell task automation.

## Features

- **Static typing with type inference** - Hindley-Milner type inference catches errors at compile time
- **Algebraic Data Types** - Define custom types with pattern matching
- **First-class functions** - Lambda expressions, closures, and higher-order functions
- **Shell integration** - Execute shell commands with structured result capture
- **Multiple execution modes** - Interpret directly or transpile to zsh scripts
- **Interactive REPL** - TUI-based REPL with history and type introspection

## Installation

### Building from Source

Requires Rust 2024 edition or later.

```bash
git clone https://github.com/ryanoneill/neap.git
cd neap
cargo build --release
```

The binary will be available at `target/release/neap`.

## Usage

### Commands

```bash
neap run <file>              # Run a Neap program
neap gen <file> [-o output]  # Transpile to zsh script
neap repl                    # Start interactive REPL
neap check <file>            # Type check without running
neap parse <file>            # Parse and print AST
neap lex <file>              # Tokenize and print tokens
```

### Quick Start

Create a file `hello.neap`:

```neap
let greeting = "Hello, World!"
let _ = print greeting
```

Run it:

```bash
neap run hello.neap
```

Or generate a shell script:

```bash
neap gen hello.neap -o hello.sh
chmod +x hello.sh
./hello.sh
```

## Language Overview

### Variables and Types

```neap
let x = 42              # int
let pi = 3.14           # float
let name = "neap"       # string
let active = true       # bool
let nothing = ()        # unit
```

Type annotations are optional but supported:

```neap
let x: int = 42
let greet: string -> string = (name) => "Hello, " ^ name
```

### Functions

```neap
fn double n = n * 2

fn factorial n =
    if n == 0 then 1
    else n * factorial (n - 1)

fn greet name = "Hello, " ^ name ^ "!"
```

Lambda expressions use Scala-style syntax:

```neap
let add = (x, y) => x + y
let increment = (n) => n + 1
```

### Pattern Matching

```neap
fn describe opt =
    match opt {
      | None -> "nothing"
      | Some x -> "got: " ^ intToString x
    }

fn length lst =
    match lst {
      | [] -> 0
      | x :: xs -> 1 + length xs
    }
```

### Algebraic Data Types

```neap
type Option<'a> =
    | None
    | Some 'a

type Result<'a, 'e> =
    | Ok 'a
    | Error 'e

type List<'a> =
    | Nil
    | Cons 'a List<'a>
```

### Shell Commands

Execute shell commands using backticks with variable interpolation:

```neap
let name = "world"
let result = `echo Hello, {name}!`
let output = result.stdout
let code = result.exitCode
let _ = print output
```

Pipe composition:

```neap
let result = `cat file.txt` |> `grep pattern` |> `wc -l`
let count = result.stdout
```

### Built-in Types

| Type | Description |
|------|-------------|
| `int` | 64-bit signed integer |
| `float` | 64-bit floating point |
| `bool` | Boolean (`true`/`false`) |
| `string` | UTF-8 string |
| `char` | Unicode character |
| `unit` | Unit type (`()`) |
| `list<'a>` | Polymorphic list |
| `option<'a>` | Optional value (`Some`/`None`) |
| `result<'a, 'e>` | Result type (`Ok`/`Error`) |

### Built-in Functions

```neap
print s              # Print string to stdout
intToString n        # Convert int to string
floatToString f      # Convert float to string
```

### REPL

Start the interactive REPL:

```bash
neap repl
```

Example session:

```
it> 1 + 2
it: int = 3

it> let x = 42
x: int = 42

it> fn double n = n * 2
double: int -> int

it> double x
it: int = 84

it> :type (x) => x + 1
int -> int
```

REPL commands:
- `:type <expr>` - Show type of expression without evaluating
- `:quit` or `:q` - Exit the REPL

## Project Structure

```
src/
├── syntax/      # Lexer, parser, AST
├── types/       # Type system and inference
├── ir/          # Intermediate representation (A-Normal Form)
├── vm/          # Tree-walking interpreter
├── codegen/     # Zsh code generation
└── repl/        # Interactive REPL
```

## Architecture

Neap follows a multi-stage compilation pipeline:

1. **Lexing** - Tokenization of source code
2. **Parsing** - Recursive descent parser produces AST
3. **Type Checking** - Hindley-Milner type inference
4. **Lowering** - Transform to A-Normal Form IR
5. **Execution** - Either interpret via VM or generate zsh code

## License

MIT License - see [LICENSE](LICENSE) for details.

# namegen

Most "random name generator" tools are either a hardcoded list you outgrow in
a week, or a whole library pulled in to shuffle a few strings around. This is
a single binary that reads a small text file describing how your names are
built, and generates as many as you want from it. No dependencies, no build
step beyond `cargo build`.

## Usage

```
$ cargo build --release
$ ./target/release/namegen examples/fantasy.grammar -n 5
Eddard Stark
Arya the Wanderer
Tyrion Lannister
Daenerys Targaryen
Cersei the Bold
```

```
usage: namegen <grammar-file> [options]

options:
  -n, --count <N>   how many names to generate (default: 5)
      --seed <N>    fix the random seed for reproducible output
      --root <name> which rule to start from (default: root)
  -h, --help        show this message
```

## Grammar files

A grammar is a set of rules. Each rule is a name followed by `:` and one or
more alternatives separated by `|`. An alternative is a sequence of quoted
string literals and `<other-rule>` references:

```
root: <first> " " <last>

first: "Bran" | "Eddard" | "Cersei"

last: "Stark" | "Lannister" | "Baratheon"
```

Generation always starts at the rule named `root` (or whatever you pass to
`--root`). References can point to rules defined later in the file. Lines
starting with `#` are comments, and alternatives can be split across lines
as long as the continuation starts with `|`:

```
first: "Bran"
     | "Eddard"
     | "Cersei"
```

An alternative can carry a weight, written `:N` anywhere in it, to make it
more or less likely than its unweighted siblings (which default to weight 1):

```
last: "Stark":3 | "Lannister" | "Targaryen":2
```

Here `"Stark"` is picked three times as often as `"Lannister"`, and twice as
often as `"Targaryen"`.

See `examples/fantasy.grammar` for a complete one.

## Error messages

Grammar files are hand-written, so they get typos. The parser tracks the
exact line and column of every token, so a mistake gets pointed at directly
instead of a vague "parse failed somewhere":

```
$ namegen broken.grammar
error: unterminated rule reference, expected a closing '>'
  |
1 | root: <first> " " <last
  |                   ^
  --> broken.grammar:1:19
```

Same treatment for undefined rules, unterminated strings, duplicate rule
names, and missing colons.

## License

MIT, see [LICENSE](LICENSE).

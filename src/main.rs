mod parser;

use parser::{Alternative, Grammar, Part};
use std::env;
use std::fs;
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

/// A tiny splitmix64 PRNG. Not cryptographic, just enough spread to pick
/// alternatives without pulling in a dependency for it.
struct Rng {
    state: u64,
}

impl Rng {
    fn new(seed: u64) -> Self {
        Rng {
            state: seed ^ 0x9E37_79B9_7F4A_7C15,
        }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next_u64() % bound as u64) as usize
    }
}

fn random_seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is before the unix epoch")
        .as_nanos() as u64
}

struct Options {
    path: String,
    count: usize,
    seed: Option<u64>,
    root: String,
}

fn parse_args() -> Result<Options, String> {
    let mut args = env::args().skip(1);
    let mut path = None;
    let mut count = 5usize;
    let mut seed = None;
    let mut root = "root".to_string();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-n" | "--count" => {
                let value = args.next().ok_or("--count needs a number after it")?;
                count = value
                    .parse()
                    .map_err(|_| format!("--count expects a number, got '{}'", value))?;
            }
            "--seed" => {
                let value = args.next().ok_or("--seed needs a number after it")?;
                seed = Some(
                    value
                        .parse()
                        .map_err(|_| format!("--seed expects a number, got '{}'", value))?,
                );
            }
            "--root" => {
                root = args.next().ok_or("--root needs a rule name after it")?;
            }
            "-h" | "--help" => {
                print_usage();
                process::exit(0);
            }
            other if path.is_none() => path = Some(other.to_string()),
            other => return Err(format!("unrecognized argument '{}'", other)),
        }
    }

    let path = path.ok_or_else(|| "missing grammar file argument".to_string())?;
    Ok(Options {
        path,
        count,
        seed,
        root,
    })
}

fn print_usage() {
    println!("namegen - generate random names from a grammar file");
    println!();
    println!("usage: namegen <grammar-file> [options]");
    println!();
    println!("options:");
    println!("  -n, --count <N>   how many names to generate (default: 5)");
    println!("      --seed <N>    fix the random seed for reproducible output");
    println!("      --root <name> which rule to start from (default: root)");
    println!("  -h, --help        show this message");
}

/// Picks an alternative with probability proportional to its weight.
fn choose_alternative<'a>(alternatives: &'a [Alternative], rng: &mut Rng) -> &'a Alternative {
    let total: u32 = alternatives.iter().map(|alt| alt.weight).sum();
    let mut pick = rng.below(total as usize) as u32;
    for alt in alternatives {
        if pick < alt.weight {
            return alt;
        }
        pick -= alt.weight;
    }
    alternatives.last().expect("grammar rules always have at least one alternative")
}

fn expand(grammar: &Grammar, rule: &str, rng: &mut Rng, depth: usize) -> Result<String, String> {
    if depth > 64 {
        return Err(format!(
            "rule '{}' is too deeply nested (possible cycle between rules)",
            rule
        ));
    }

    let alternatives = grammar
        .get(rule)
        .ok_or_else(|| format!("rule '{}' is not defined", rule))?;

    let choice = choose_alternative(alternatives, rng);
    let mut out = String::new();
    for part in &choice.parts {
        match part {
            Part::Literal(text) => out.push_str(text),
            Part::Reference { name, .. } => {
                out.push_str(&expand(grammar, name, rng, depth + 1)?);
            }
        }
    }
    Ok(out)
}

fn main() {
    let options = match parse_args() {
        Ok(options) => options,
        Err(message) => {
            eprintln!("error: {}", message);
            eprintln!();
            print_usage();
            process::exit(2);
        }
    };

    let source = match fs::read_to_string(&options.path) {
        Ok(source) => source,
        Err(err) => {
            eprintln!("error: could not read '{}': {}", options.path, err);
            process::exit(1);
        }
    };

    let grammar = match parser::parse(&source) {
        Ok(grammar) => grammar,
        Err(err) => {
            eprintln!("{}", err.render(&source));
            eprintln!("  --> {}:{}:{}", options.path, err.line, err.col);
            process::exit(1);
        }
    };

    let mut rng = Rng::new(options.seed.unwrap_or_else(random_seed));

    for _ in 0..options.count {
        match expand(&grammar, &options.root, &mut rng, 0) {
            Ok(name) => println!("{}", name),
            Err(message) => {
                eprintln!("error: {}", message);
                process::exit(1);
            }
        }
    }
}

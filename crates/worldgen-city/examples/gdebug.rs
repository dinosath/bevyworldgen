use symbios_shape::grammar::parse_rule;
use symbios_shape::{Interpreter, Quat, Scope, Vec3};
fn main() {
    for (name, kind) in [
        ("house", worldgen_city::styles::Kind::House),
        ("shop", worldgen_city::styles::Kind::Shop),
        ("tower", worldgen_city::styles::Kind::Tower),
        ("barn", worldgen_city::styles::Kind::Barn),
    ] {
        let g = worldgen_city::styles::grammar(
            kind,
            worldgen_city::styles::Region::Temperate,
            (30, 50),
        );
        let mut i = Interpreter::new();
        for line in g.lines().map(str::trim).filter(|l| !l.is_empty()) {
            match parse_rule(line) {
                Ok(r) => {
                    if let Err(e) = i.add_grammar_rule(r) {
                        println!("{name}: add error {e:?} for {line}");
                    }
                }
                Err(e) => println!("{name}: PARSE ERROR {e:?}\n   {line}"),
            }
        }
        i.seed = 7;
        match i.derive(
            Scope::new(Vec3::ZERO, Quat::IDENTITY, Vec3::new(16.0, 0.0, 22.0)),
            "Lot",
        ) {
            Ok(m) => println!("{name}: {} terminals", m.terminals.len()),
            Err(e) => println!("{name}: DERIVE ERROR {e:?}"),
        }
    }
}

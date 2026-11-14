fn main() {
    for (l, x, z) in wasm_golden::PROBE {
        println!("{l} {x} {z} {:016x}", wasm_golden::golden_native(l, x, z));
    }
}

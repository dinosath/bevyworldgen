// Instantiates the wasm32 build and prints the same probe fingerprints as the native binary.
import { readFileSync } from "node:fs";
const bytes = readFileSync(process.argv[2]);
const { instance } = await WebAssembly.instantiate(bytes, {});
const probe = [[0,0,0],[0,-7,3],[1,12,-9],[2,5,5],[5,-1,-1],[7,2,2],[0,415,741],[0,673,601]];
for (const [l, x, z] of probe) {
  const h = instance.exports.golden(l, x, z);
  console.log(`${l} ${x} ${z} ${BigInt.asUintN(64, h).toString(16).padStart(16, "0")}`);
}

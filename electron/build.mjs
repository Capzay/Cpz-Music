import fs from "node:fs";
import path from "node:path";

/**
 * Electron's main process needs CommonJS. tsc emits .js, which Node treats as
 * ESM or CJS depending on the nearest package.json "type", so rename to .cjs to
 * settle it, and fix the requires that pointed at the old names.
 */
const dist = "dist";

for (const file of fs.readdirSync(dist)) {
  if (!file.endsWith(".js")) continue;

  const from = path.join(dist, file);
  const to = path.join(dist, file.replace(/\.js$/, ".cjs"));
  let source = fs.readFileSync(from, "utf8");
  source = source.replace(/require\("\.\/([^"]+)"\)/g, 'require("./$1.cjs")');

  fs.writeFileSync(to, source);
  fs.unlinkSync(from);
}

// Already CommonJS, so it is copied rather than compiled.
fs.copyFileSync("preload.cjs", path.join(dist, "preload.cjs"));

console.log("electron: build output ready in dist/");

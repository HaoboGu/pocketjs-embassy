import { mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
const root = resolve(import.meta.dir, "..");
const theme = JSON.parse(readFileSync(resolve(root, "ui/themes/github-light.json"), "utf8"));
const stage = resolve(root, "target/ui-source");
mkdirSync(resolve(stage, "components"), { recursive: true });
for (const file of ["app.tsx", "Screen.tsx", "Status.ts", ...readdirSync(resolve(root, "ui/components")).filter(f => f.endsWith(".tsx")).map(f => `components/${f}`)]) {
  const source = readFileSync(resolve(root, "ui", file), "utf8").replace(/\b(bg|text|border|from|via|to)-theme-([a-z0-9]+)\b/g, (_, kind, key) => {
    const color = theme.colors[key];
    if (!/^#[0-9a-fA-F]{6}$/.test(color)) throw new Error(`Unknown theme color ${key}`);
    return `${kind}-[${color}]`;
  });
  writeFileSync(resolve(stage, file), source);
}
writeFileSync(resolve(stage, "pocket.app.json"), JSON.stringify({ app: { entry: "app.tsx", framework: "solid", theme: JSON.parse(readFileSync(resolve(root, "ui/motion.json"), "utf8")) } }));
const result = Bun.spawnSync(["bun", resolve(root, ".deps/pocketjs/tools/build.ts"), resolve(stage, "app.tsx"), "--strict-classes", "--extra-chars=" + Array.from({ length: 95 }, (_, i) => String.fromCharCode(i + 32)).join(""), `--project-root=${stage}`, `--outdir=${resolve(root, "ui/generated")}`], { cwd: root, env: { ...process.env, POCKETJS_MINIFY: "1" }, stdout: "inherit", stderr: "inherit" });
process.exit(result.exitCode);

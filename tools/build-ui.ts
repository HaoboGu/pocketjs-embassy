import { mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
const root = resolve(import.meta.dir, "..");
const args = process.argv.slice(2);
const option = (name: string, fallback: string) => {
  const index = args.indexOf(name);
  if (index < 0) return fallback;
  if (!args[index + 1]) throw new Error(`Missing value for ${name}`);
  return args[index + 1];
};
const layout = option("--layout", "github");
if (!["github", "paperline"].includes(layout)) throw new Error(`Unknown layout ${layout}`);
const outdir = resolve(root, option("--outdir", layout === "github" ? "ui/generated" : `target/ui/${layout}`));
const theme = JSON.parse(readFileSync(resolve(root, "ui/themes/github-light.json"), "utf8"));
const stage = resolve(root, `target/ui-source-${layout}`);
mkdirSync(resolve(stage, "components"), { recursive: true });
for (const file of ["app.tsx", "Screen.tsx", "Status.ts", ...readdirSync(resolve(root, "ui/components")).filter(f => f.endsWith(".tsx")).map(f => `components/${f}`)]) {
  const source = readFileSync(resolve(root, "ui", layout !== "github" && file === "Screen.tsx" ? `layouts/${layout}/${file}` : file), "utf8").replace(/\b(bg|text|border|from|via|to)-theme-([a-z0-9]+)\b/g, (_, kind, key) => {
    const color = theme.colors[key];
    if (!/^#[0-9a-fA-F]{6}$/.test(color)) throw new Error(`Unknown theme color ${key}`);
    return `${kind}-[${color}]`;
  });
  writeFileSync(resolve(stage, file), source);
}
const motion = JSON.parse(readFileSync(resolve(root, "ui/motion.json"), "utf8"));
if (layout === "paperline") {
  motion.keyframes["paper-ink"] = { from: { transform: "translateX(0px)", opacity: 0.4 }, to: { transform: "translateX(320px)", opacity: 1 } };
  motion.keyframes["paper-charge"] = { from: { transform: "translateX(0px)" }, to: { transform: "translateX(144px)" } };
  motion.animation["paper-ink"] = "paper-ink 1200ms ease-in-out infinite";
  motion.animation["paper-charge"] = "paper-charge 2400ms ease-in-out infinite";
}
writeFileSync(resolve(stage, "pocket.app.json"), JSON.stringify({ app: { entry: "app.tsx", framework: "solid", theme: motion } }));
const result = Bun.spawnSync(["bun", resolve(root, ".deps/pocketjs/tools/build.ts"), resolve(stage, "app.tsx"), "--strict-classes", "--extra-chars=" + Array.from({ length: 95 }, (_, i) => String.fromCharCode(i + 32)).join(""), `--project-root=${stage}`, `--outdir=${outdir}`], { cwd: root, env: { ...process.env, POCKETJS_MINIFY: "1" }, stdout: "inherit", stderr: "inherit" });
if (result.exitCode === 0) {
  const spec = readFileSync(resolve(root, ".deps/pocketjs/engine/spec/src/lib.rs"), "utf8");
  const abi = Number(spec.match(/BINDING_GENERATION: u32 = (\d+)/)![1]);
  writeFileSync(resolve(outdir, "app.meta.json"), JSON.stringify({ abi, layout, viewport: [480, 480] }, null, 2) + "\n");
}
process.exit(result.exitCode);

import { mkdir } from "node:fs/promises";
import path from "node:path";

export const TARGETS = {
  macosIntel: "bun-darwin-x64",
  macosArm: "bun-darwin-arm64",
  windowsX64: "bun-windows-x64",
} as const;

export type SidecarTarget = (typeof TARGETS)[keyof typeof TARGETS];

const ARTIFACT_NAME_BY_TARGET: Record<SidecarTarget, string> = {
  [TARGETS.macosIntel]: "engine-x86_64-apple-darwin",
  [TARGETS.macosArm]: "engine-aarch64-apple-darwin",
  [TARGETS.windowsX64]: "engine-x86_64-pc-windows-msvc.exe",
};

export function artifactNameForTarget(target: SidecarTarget): string {
  return ARTIFACT_NAME_BY_TARGET[target];
}

export interface BuildPlan {
  command: string[];
  cwd: string;
  outfile: string;
}

export function planBuild(options: {
  target: SidecarTarget;
  rootDir: string;
}): BuildPlan {
  const binariesDir = path.join(options.rootDir, "src-tauri", "binaries");
  const outfile = path.join(
    binariesDir,
    artifactNameForTarget(options.target),
  );

  return {
    cwd: path.join(options.rootDir, "src-sidecar"),
    outfile,
    command: [
      "bun",
      "build",
      "src/server.ts",
      "--compile",
      "--target",
      options.target,
      "--outfile",
      outfile,
    ],
  };
}

async function runPlan(plan: BuildPlan): Promise<void> {
  await mkdir(path.dirname(plan.outfile), { recursive: true });
  const proc = Bun.spawn(plan.command, {
    cwd: plan.cwd,
    stdout: "inherit",
    stderr: "inherit",
  });
  const exitCode = await proc.exited;
  if (exitCode !== 0) {
    throw new Error(`build failed: ${plan.command.join(" ")}`);
  }
}

function parseTargets(args: string[]): SidecarTarget[] {
  if (args.includes("--all")) {
    return [TARGETS.macosIntel, TARGETS.macosArm, TARGETS.windowsX64];
  }

  const targetArgIndex = args.findIndex((arg) => arg === "--target");
  if (targetArgIndex >= 0) {
    const value = args[targetArgIndex + 1] as SidecarTarget | undefined;
    if (value && value in ARTIFACT_NAME_BY_TARGET) {
      return [value];
    }
    throw new Error("invalid --target value");
  }

  return [TARGETS.macosIntel];
}

async function main() {
  const args = process.argv.slice(2);
  const dryRun = args.includes("--dry-run");
  const rootDir = path.resolve(import.meta.dir, "..", "..");
  const targets = parseTargets(args);

  for (const target of targets) {
    const plan = planBuild({ target, rootDir });
    if (dryRun) {
      console.log(JSON.stringify(plan, null, 2));
      continue;
    }
    await runPlan(plan);
  }
}

if (import.meta.main) {
  await main();
}

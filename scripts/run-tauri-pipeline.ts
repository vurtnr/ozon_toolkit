import { spawn } from "node:child_process";
import path from "node:path";

function runCommand(command: string[], cwd: string): Promise<void> {
  return new Promise((resolve, reject) => {
    const child = spawn(command[0]!, command.slice(1), {
      cwd,
      stdio: "inherit",
      env: process.env,
    });

    child.once("error", reject);
    child.once("exit", (code, signal) => {
      if (code === 0) {
        resolve();
        return;
      }
      reject(
        new Error(
          `command failed (${command.join(" ")}), code=${code ?? "null"}, signal=${signal ?? "null"}`,
        ),
      );
    });
  });
}

async function main(): Promise<void> {
  const mode = process.argv[2];
  if (mode !== "dev" && mode !== "build") {
    throw new Error('usage: bun run scripts/run-tauri-pipeline.ts <dev|build>');
  }

  const rootDir = path.resolve(import.meta.dir, "..");
  await runCommand(
    ["bun", "run", "scripts/build-sidecar.ts", "--host"],
    path.join(rootDir, "src-sidecar"),
  );

  if (mode === "dev") {
    await runCommand(["bun", "run", "dev"], rootDir);
    return;
  }

  await runCommand(["bun", "run", "build"], rootDir);
}

await main();

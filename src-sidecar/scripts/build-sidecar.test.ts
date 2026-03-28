import { readFileSync } from "node:fs";
import path from "node:path";
import { describe, expect, test } from "bun:test";
import {
  TARGETS,
  artifactNameForTarget,
  planBuild,
  resolveHostTarget,
} from "./build-sidecar";

describe("sidecar build target mapping", () => {
  test("maps macOS intel target to expected artifact name", () => {
    expect(artifactNameForTarget(TARGETS.macosIntel)).toBe(
      "engine-x86_64-apple-darwin",
    );
  });

  test("maps macOS arm target to expected artifact name", () => {
    expect(artifactNameForTarget(TARGETS.macosArm)).toBe(
      "engine-aarch64-apple-darwin",
    );
  });

  test("maps windows target to expected artifact name", () => {
    expect(artifactNameForTarget(TARGETS.windowsX64)).toBe(
      "engine-x86_64-pc-windows-msvc.exe",
    );
  });

  test("produces compile command plan for dry run", () => {
    const plan = planBuild({
      target: TARGETS.windowsX64,
      rootDir: "/tmp/desktop_app",
    });

    expect(plan.outfile).toBe(
      "/tmp/desktop_app/src-tauri/binaries/engine-x86_64-pc-windows-msvc.exe",
    );
    expect(plan.command).toEqual([
      "bun",
      "build",
      "src/server.ts",
      "--compile",
      "--target",
      "bun-windows-x64",
      "--outfile",
      "/tmp/desktop_app/src-tauri/binaries/engine-x86_64-pc-windows-msvc.exe",
    ]);
  });

  test("resolves macOS arm host to arm sidecar target", () => {
    expect(resolveHostTarget("darwin", "arm64")).toBe(TARGETS.macosArm);
  });

  test("resolves macOS intel host to intel sidecar target", () => {
    expect(resolveHostTarget("darwin", "x64")).toBe(TARGETS.macosIntel);
  });

  test("resolves windows x64 host to windows sidecar target", () => {
    expect(resolveHostTarget("win32", "x64")).toBe(TARGETS.windowsX64);
  });

  test("sidecar package avoids compile-incompatible puppeteer extra plugins", () => {
    const packageJsonPath = path.join(import.meta.dir, "..", "package.json");
    const serverPath = path.join(import.meta.dir, "..", "src", "server.ts");
    const packageJson = JSON.parse(readFileSync(packageJsonPath, "utf8")) as {
      dependencies?: Record<string, string>;
    };
    const serverSource = readFileSync(serverPath, "utf8");

    expect(packageJson.dependencies?.["puppeteer-extra"]).toBeUndefined();
    expect(
      packageJson.dependencies?.["puppeteer-extra-plugin-stealth"],
    ).toBeUndefined();
    expect(serverSource).not.toContain("puppeteer-extra");
    expect(serverSource).not.toContain("puppeteer-extra-plugin-stealth");
    expect(serverSource).toContain("SIDECAR_PROFILE_DIR");
    expect(serverSource).not.toContain('path.resolve(process.cwd(), "1688_profile")');
  });
});

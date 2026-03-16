import { access } from "node:fs/promises";
import { constants } from "node:fs";
import { ERROR_CODES } from "./error-codes";

export class ChromeNotFoundError extends Error {
  readonly code = ERROR_CODES.CHROME_NOT_FOUND;

  constructor(message: string = "Chrome executable was not found") {
    super(message);
    this.name = "ChromeNotFoundError";
  }
}

type ExistsFn = (candidate: string) => boolean | Promise<boolean>;

export interface FindChromePathOptions {
  env?: Record<string, string | undefined>;
  exists?: ExistsFn;
  platform?: NodeJS.Platform;
}

const defaultExists: ExistsFn = async (candidate: string) => {
  try {
    await access(candidate, constants.F_OK);
    return true;
  } catch {
    return false;
  }
};

function pushIfPresent(list: string[], value: string | undefined): void {
  if (!value) return;
  const trimmed = value.trim();
  if (trimmed.length > 0) list.push(trimmed);
}

function buildPlatformCandidates(
  env: Record<string, string | undefined>,
  platform: NodeJS.Platform,
): string[] {
  const candidates: string[] = [];

  if (platform === "win32") {
    const programFiles = env.ProgramFiles || "C:\\Program Files";
    const programFilesX86 = env["ProgramFiles(x86)"] || "C:\\Program Files (x86)";
    const localAppData = env.LOCALAPPDATA;

    candidates.push(
      `${programFiles}\\Google\\Chrome\\Application\\chrome.exe`,
      `${programFilesX86}\\Google\\Chrome\\Application\\chrome.exe`,
    );
    pushIfPresent(
      candidates,
      localAppData
        ? `${localAppData}\\Google\\Chrome\\Application\\chrome.exe`
        : undefined,
    );
    return candidates;
  }

  if (platform === "darwin") {
    candidates.push(
      "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
      "/Applications/Google Chrome Canary.app/Contents/MacOS/Google Chrome Canary",
    );
    return candidates;
  }

  candidates.push(
    "/usr/bin/google-chrome",
    "/usr/bin/chromium-browser",
    "/snap/bin/chromium",
  );

  return candidates;
}

export async function findChromePath(
  options: FindChromePathOptions = {},
): Promise<string> {
  const env = options.env ?? process.env;
  const exists = options.exists ?? defaultExists;
  const platform = options.platform ?? process.platform;

  const candidates: string[] = [];
  pushIfPresent(candidates, env.CHROME_EXECUTABLE_PATH);
  candidates.push(...buildPlatformCandidates(env, platform));

  for (const candidate of candidates) {
    if (await exists(candidate)) {
      return candidate;
    }
  }

  throw new ChromeNotFoundError();
}

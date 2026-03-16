import { describe, expect, test } from "bun:test";
import { shutdownRuntimeResources } from "./server";

describe("shutdownRuntimeResources", () => {
  test("closes page, browser and http server in order", async () => {
    const calls: string[] = [];

    await shutdownRuntimeResources({
      page: {
        isClosed: () => false,
        close: async () => {
          calls.push("page");
        },
      },
      browser: {
        isConnected: () => true,
        close: async () => {
          calls.push("browser");
        },
      },
      server: {
        close: (callback) => {
          calls.push("server");
          callback();
          return {} as never;
        },
      },
    });

    expect(calls).toEqual(["page", "browser", "server"]);
  });

  test("skips already-closed resources", async () => {
    const calls: string[] = [];

    await shutdownRuntimeResources({
      page: {
        isClosed: () => true,
        close: async () => {
          calls.push("page");
        },
      },
      browser: {
        isConnected: () => false,
        close: async () => {
          calls.push("browser");
        },
      },
      server: null,
    });

    expect(calls).toEqual([]);
  });
});

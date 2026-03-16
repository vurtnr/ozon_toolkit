import { describe, expect, test } from "bun:test";
import {
  buildRunTaskPayload,
  formatFileSize,
  extractDisplayFileName,
  isAbsoluteXlsxPath,
  isXlsxPath,
  normalizeDroppedPath,
  pickFirstUriPath,
  shouldEnableRun,
} from "../../composables/useTaskRunner";

describe("Task runner helpers", () => {
  test("accepts absolute .xlsx paths on macOS and Windows", () => {
    expect(isAbsoluteXlsxPath("/Users/demo/input.xlsx")).toBe(true);
    expect(isAbsoluteXlsxPath("C:\\work\\input.xlsx")).toBe(true);
  });

  test("rejects relative or non-xlsx paths", () => {
    expect(isAbsoluteXlsxPath("./input.xlsx")).toBe(false);
    expect(isAbsoluteXlsxPath("/Users/demo/input.csv")).toBe(false);
  });

  test("normalizes file URI dropped path", () => {
    expect(normalizeDroppedPath("file:///Users/demo/input.xlsx")).toBe(
      "/Users/demo/input.xlsx",
    );
  });

  test("identifies xlsx file extension", () => {
    expect(isXlsxPath("/Users/demo/input.xlsx")).toBe(true);
    expect(isXlsxPath("/Users/demo/input.xls")).toBe(false);
  });

  test("extracts display file name from path", () => {
    expect(extractDisplayFileName("/Users/demo/input.xlsx")).toBe("input.xlsx");
    expect(extractDisplayFileName("C:\\demo\\input.xlsx")).toBe("input.xlsx");
  });

  test("picks first path from uri-list payload", () => {
    const uriList = "# comment\nfile:///Users/demo/input.xlsx\n";
    expect(pickFirstUriPath(uriList)).toBe("file:///Users/demo/input.xlsx");
  });

  test("enables run only when upload has completed", () => {
    expect(shouldEnableRun(false, false, "/tmp/uploaded.xlsx")).toBe(true);
    expect(shouldEnableRun(false, true, "/tmp/uploaded.xlsx")).toBe(false);
    expect(shouldEnableRun(true, false, "/tmp/uploaded.xlsx")).toBe(false);
    expect(shouldEnableRun(false, false, "")).toBe(false);
  });

  test("formats file size text", () => {
    expect(formatFileSize(0)).toBe("0 B");
    expect(formatFileSize(1024)).toBe("1.0 KB");
    expect(formatFileSize(5 * 1024 * 1024)).toBe("5.0 MB");
  });

  test("builds invoke payload shape", () => {
    expect(
      buildRunTaskPayload("/tmp/uploaded-input.xlsx", "/Users/demo/input.xlsx"),
    ).toEqual({
      excelPath: "/tmp/uploaded-input.xlsx",
      sourceExcelPath: "/Users/demo/input.xlsx",
    });
    expect(buildRunTaskPayload("/Users/demo/input.xlsx")).toEqual({
      excelPath: "/Users/demo/input.xlsx",
    });
  });
});

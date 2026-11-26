import { describe, expect, it } from "vitest";
import { joinList, parseList, stripHandle } from "./lists";

describe("parseList", () => {
  it("splits on commas, semicolons and newlines", () => {
    expect(parseList("rust, tauri;sqlite\nlocal-first")).toEqual(["rust", "tauri", "sqlite", "local-first"]);
  });

  it("trims, drops empties and de-duplicates case-insensitively", () => {
    expect(parseList("  AI ,, ai , Rust ,")).toEqual(["AI", "Rust"]);
  });

  it("returns an empty list for blank input", () => {
    expect(parseList("  \n ")).toEqual([]);
  });
});

describe("helpers", () => {
  it("joins for display", () => {
    expect(joinList(["a", "b"])).toBe("a, b");
  });

  it("strips leading @", () => {
    expect(stripHandle(" @@naval ")).toBe("naval");
  });
});

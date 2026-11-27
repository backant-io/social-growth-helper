import { describe, expect, it } from "vitest";
import { externalLinkTarget, isXPostLink } from "./api";

describe("externalLinkTarget", () => {
  it("accepts new-window web links and ignores everything else", () => {
    expect(externalLinkTarget({ target: "_blank", href: "https://x.com/naval/status/111" })).toBe("https://x.com/naval/status/111");
    expect(externalLinkTarget({ target: "_blank", href: "http://example.com" })).toBe("http://example.com");
    expect(externalLinkTarget({ target: "", href: "https://x.com/a/status/1" })).toBeNull();
    expect(externalLinkTarget({ target: "_blank", href: "data:image/svg+xml;utf8,x" })).toBeNull();
    expect(externalLinkTarget({ target: "_blank", href: "javascript:alert(1)" })).toBeNull();
    expect(externalLinkTarget(null)).toBeNull();
  });
});

describe("isXPostLink", () => {
  it("recognises post permalinks only", () => {
    expect(isXPostLink("https://x.com/naval/status/111")).toBe(true);
    expect(isXPostLink("https://twitter.com/naval/status/111?s=20")).toBe(true);
    expect(isXPostLink("https://x.com/i/status/111")).toBe(true);
    expect(isXPostLink("https://x.com/naval")).toBe(false);
    expect(isXPostLink("https://example.com/naval/status/111")).toBe(false);
    expect(isXPostLink("https://x.com.evil.com/naval/status/111")).toBe(false);
  });
});

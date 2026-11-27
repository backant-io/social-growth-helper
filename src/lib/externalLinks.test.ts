import { describe, expect, it } from "vitest";
import { externalLinkTarget } from "./api";

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

import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { XPostCard } from "./XPostCard";

const metrics = { replies: 1, reposts: 2, likes: 3, bookmarks: 4, views: 5 };

describe("XPostCard", () => {
  it("always renders a visible View on X link to the permalink", () => {
    const html = renderToStaticMarkup(
      <XPostCard handle="naval" text="hi" postedAt={null} metrics={metrics} url="https://x.com/naval/status/111">
        <span>reply box</span>
      </XPostCard>,
    );
    expect(html).toContain('data-testid="view-on-x"');
    expect(html).toContain('href="https://x.com/naval/status/111"');
    expect(html).toContain("View on X");
    expect(html).toContain('target="_blank"');
  });

  it("omits the link only when there is no URL", () => {
    expect(renderToStaticMarkup(<XPostCard handle="a" text="x" metrics={metrics} url={null} />)).not.toContain("view-on-x");
  });
});

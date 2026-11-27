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

  it("shows the score as one quiet line and puts View on X after the metrics, pushed right", () => {
    const html = renderToStaticMarkup(<XPostCard handle="a" text="x" metrics={metrics} url="https://x.com/a/status/1" signal="Score 91 · High saves" />);
    expect(html).toContain('data-testid="post-signal"');
    expect(html).toContain("Score 91 · High saves");
    expect(html.indexOf("Engagement")).toBeLessThan(html.indexOf("view-on-x"));
    expect(html.match(/<a [^>]*data-testid="view-on-x"[^>]*>/)?.[0]).toContain("ml-auto");
    expect(renderToStaticMarkup(<XPostCard handle="a" text="x" metrics={metrics} />)).not.toContain("post-signal");
  });
});

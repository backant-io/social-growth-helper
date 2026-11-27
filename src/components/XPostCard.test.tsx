import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { RepostMenu, XPostCard } from "./XPostCard";

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

  it("keeps the repost count a plain stat when no repost handler is given", () => {
    const html = renderToStaticMarkup(<XPostCard handle="a" text="x" metrics={metrics} />);
    expect(html).not.toContain("repost-trigger");
  });

  it("turns the repost icon into a menu trigger when a handler is given", () => {
    const html = renderToStaticMarkup(<XPostCard handle="a" text="x" metrics={metrics} onRepostChoice={() => undefined} />);
    expect(html).toContain('data-testid="repost-trigger"');
    expect(html).toContain('aria-haspopup="menu"');
    expect(html).toContain('aria-expanded="false"');
    expect(html).not.toContain('data-testid="repost-menu"');
  });
});

describe("RepostMenu", () => {
  it("opens with the two X-like choices: Repost and Quote", () => {
    const html = renderToStaticMarkup(<RepostMenu count={2} onChoose={() => undefined} defaultOpen />);
    expect(html).toContain('role="menu"');
    const items = [...html.matchAll(/role="menuitem"[^>]*data-testid="repost-menu-(\w+)"/g)].map((m) => m[1]);
    expect(items).toEqual(["repost", "quote"]);
    expect(html).toContain("Repost");
    expect(html).toContain("Quote");
    expect(html).not.toContain("Reply");
  });

  it("offers Undo repost in place of Repost when already reposted", () => {
    const html = renderToStaticMarkup(<RepostMenu count={2} reposted onChoose={() => undefined} defaultOpen />);
    expect(html).toContain("Undo repost");
    expect(html).toContain("Quote");
  });
});

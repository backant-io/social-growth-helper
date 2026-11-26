import type { SampleKind } from "./tourSteps";

const card = "rounded-xl border border-zinc-200 bg-zinc-50 p-4 text-sm dark:border-zinc-800 dark:bg-zinc-900";
const chip = "rounded-full border border-zinc-300 px-2.5 py-1 text-xs dark:border-zinc-700";

function Sample({ children }: { children: React.ReactNode }) {
  return (
    <div className={card} data-testid="tour-sample">
      <div className="mb-2 text-[10px] font-semibold uppercase tracking-wider text-amber-600 dark:text-amber-400">
        Sample data
      </div>
      {children}
    </div>
  );
}

export function SampleCard({ kind }: { kind: SampleKind }) {
  switch (kind) {
    case "scout":
      return (
        <Sample>
          <ul className="space-y-2">
            {[
              ["@sample_dev", "Shipping a local-first app taught me more than any course.", "92"],
              ["@sample_founder", "What is one habit that saved your project?", "87"],
            ].map(([h, t, s]) => (
              <li key={h} className="flex items-start justify-between gap-3">
                <div>
                  <div className="font-medium">{h}</div>
                  <div className="text-zinc-600 dark:text-zinc-400">{t}</div>
                </div>
                <span className={chip}>fit {s}</span>
              </li>
            ))}
          </ul>
        </Sample>
      );
    case "reply":
      return (
        <Sample>
          <div className="mb-3 text-zinc-600 dark:text-zinc-400">
            Draft: &quot;Same here. The thing that surprised me was how much simpler sync got once the data lived on the device.&quot;
          </div>
          <div className="flex flex-wrap gap-2">
            {["Reply", "Quote", "Like", "Repost", "Bookmark", "Follow"].map((a) => (
              <button key={a} type="button" disabled className={`${chip} opacity-70`}>
                {a}
              </button>
            ))}
          </div>
        </Sample>
      );
    case "publish":
      return (
        <Sample>
          <div className="mb-2 font-medium">Ready to send as a reply to @sample_dev</div>
          <div className="mb-3 text-zinc-600 dark:text-zinc-400">You will see the exact text first. Nothing is sent until you confirm.</div>
          <button type="button" disabled className={`${chip} opacity-70`}>
            Confirm and send
          </button>
        </Sample>
      );
    case "creator":
      return (
        <Sample>
          <div className="font-medium">Format: &quot;The thing nobody tells you about X&quot;</div>
          <div className="mt-1 text-zinc-600 dark:text-zinc-400">Seed idea, outline, then your rewrite. Original posts only.</div>
        </Sample>
      );
    case "dashboard":
      return (
        <Sample>
          <div className="grid grid-cols-3 gap-3 text-center">
            {[
              ["Impressions", "12.4k"],
              ["Replies", "38"],
              ["New follows", "+21"],
            ].map(([l, v]) => (
              <div key={l}>
                <div className="text-lg font-semibold">{v}</div>
                <div className="text-xs text-zinc-500">{l}</div>
              </div>
            ))}
          </div>
        </Sample>
      );
    case "learning":
      return (
        <Sample>
          <div className="font-medium">Insight</div>
          <div className="text-zinc-600 dark:text-zinc-400">Shorter replies with one concrete detail did better. Suggest raising the weight for specificity?</div>
          <div className="mt-2 flex gap-2">
            <span className={chip}>Approve</span>
            <span className={chip}>Dismiss</span>
          </div>
        </Sample>
      );
    case "models":
      return (
        <Sample>
          <ul className="space-y-1">
            {["Scout scoring", "Reply drafting", "Quote drafting", "Creator", "Style analysis", "Learning", "Summaries"].map((t) => (
              <li key={t} className="flex justify-between">
                <span>{t}</span>
                <span className="text-zinc-500">choose a model</span>
              </li>
            ))}
          </ul>
        </Sample>
      );
  }
}

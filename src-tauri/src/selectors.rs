//! Versioned X selector registry (BRW-12). Every selector the app uses lives here; changing one means
//! editing this file only. Baseline from the Phase 0 catalogue (SPK-06); live signed-in verification is
//! still pending, so `REGISTRY_VERSION` stays at 1 until the manual checklist has been run.
//!
//! Rules: only `data-testid`, ARIA roles, href patterns and semantic attributes. Never hashed class names.

use serde_json::json;

pub const REGISTRY_VERSION: u32 = 1;

pub struct Selector {
    pub name: &'static str,
    pub primary: &'static str,
    pub fallbacks: &'static [&'static str],
}

impl Selector {
    pub fn all(&self) -> Vec<&'static str> {
        std::iter::once(self.primary).chain(self.fallbacks.iter().copied()).collect()
    }
}

macro_rules! sel {
    ($id:ident, $primary:expr, [$($fb:expr),*]) => {
        pub const $id: Selector = Selector { name: stringify!($id), primary: $primary, fallbacks: &[$($fb),*] };
    };
}

pub mod sel {
    use super::Selector;
    sel!(TWEET, "article[data-testid=\"tweet\"]", ["article[role=\"article\"]"]);
    sel!(TWEET_TEXT, "[data-testid=\"tweetText\"]", ["article div[lang][dir=\"auto\"]"]);
    sel!(AUTHOR, "[data-testid=\"User-Name\"]", []);
    sel!(PERMALINK_TIME, "a[href*=\"/status/\"] time", ["time[datetime]"]);
    sel!(PROMOTED, "[data-testid=\"placementTracking\"]", []);
    sel!(REPLY_RESTRICTED, "[data-testid=\"replyRestriction\"]", []);
    sel!(METRICS_GROUP, "[role=\"group\"][aria-label]", []);
    sel!(MEDIA, "[data-testid=\"tweetPhoto\"]", ["[data-testid=\"videoPlayer\"]"]);
    sel!(QUOTE_CARD, "[data-testid=\"quoteTweet\"]", []);
    sel!(SHOW_MORE, "[data-testid=\"tweet-text-show-more-link\"]", []);
    sel!(IMAGE, "[data-testid=\"tweetPhoto\"] img", []);
    sel!(ACCOUNT_SWITCHER, "[data-testid=\"SideNav_AccountSwitcher_Button\"]", []);
    sel!(LIKE, "[data-testid=\"like\"]", ["button[aria-label*=\"Like\"]"]);
    sel!(UNLIKE, "[data-testid=\"unlike\"]", ["button[aria-label*=\"Liked\"]"]);
    sel!(REPLY, "[data-testid=\"reply\"]", ["button[aria-label*=\"Reply\"]"]);
    sel!(RETWEET, "[data-testid=\"retweet\"]", ["button[aria-label*=\"Repost\"]"]);
    sel!(UNRETWEET, "[data-testid=\"unretweet\"]", []);
    sel!(RETWEET_CONFIRM, "[data-testid=\"retweetConfirm\"]", []);
    sel!(UNRETWEET_CONFIRM, "[data-testid=\"unretweetConfirm\"]", []);
    sel!(BOOKMARK, "[data-testid=\"bookmark\"]", ["button[aria-label*=\"Bookmark\"]"]);
    sel!(UNBOOKMARK, "[data-testid=\"removeBookmark\"]", []);
    sel!(FOLLOW, "[data-testid$=\"-follow\"]", []);
    sel!(UNFOLLOW, "[data-testid$=\"-unfollow\"]", []);
    sel!(UNFOLLOW_CONFIRM, "[data-testid=\"confirmationSheetConfirm\"]", []);
    sel!(CARET, "[data-testid=\"caret\"]", []);
    sel!(MENU_ITEM, "[role=\"menuitem\"]", []);
    sel!(COMPOSER, "[data-testid=\"tweetTextarea_0\"]", ["div[contenteditable=\"true\"][role=\"textbox\"]"]);
    sel!(SUBMIT, "[data-testid=\"tweetButton\"]", ["[data-testid=\"tweetButtonInline\"]"]);
    sel!(TOAST, "[data-testid=\"toast\"]", []);
    sel!(CHALLENGE_CLOUDFLARE, "iframe[src*=\"challenges.cloudflare.com\"]", ["#challenge-running"]);
    sel!(CHALLENGE_ARKOSE, "iframe[src*=\"arkoselabs\"]", []);
    sel!(CHALLENGE_TEXT_INPUT, "[data-testid=\"ocfEnterTextTextInput\"]", []);
}

pub const LOGGED_OUT_PATHS: &[&str] = &["/i/jf/onboarding", "/i/flow/login", "/login"];

pub fn registry() -> Vec<&'static Selector> {
    use sel::*;
    vec![
        &TWEET, &TWEET_TEXT, &AUTHOR, &PERMALINK_TIME, &PROMOTED, &REPLY_RESTRICTED, &METRICS_GROUP, &MEDIA, &QUOTE_CARD, &SHOW_MORE, &IMAGE,
        &ACCOUNT_SWITCHER, &LIKE, &UNLIKE, &REPLY, &RETWEET, &UNRETWEET, &RETWEET_CONFIRM, &UNRETWEET_CONFIRM,
        &BOOKMARK, &UNBOOKMARK, &FOLLOW, &UNFOLLOW, &UNFOLLOW_CONFIRM, &CARET, &MENU_ITEM, &COMPOSER, &SUBMIT, &TOAST,
        &CHALLENGE_CLOUDFLARE, &CHALLENGE_ARKOSE, &CHALLENGE_TEXT_INPUT,
    ]
}

/// Self-check (BRW-12): returns a problem per selector that is empty, looks like a hashed class name, or
/// is not a plausible CSS selector. An empty result means the registry is sound.
pub fn self_check() -> Vec<String> {
    let mut problems = Vec::new();
    for s in registry() {
        for css in s.all() {
            if css.trim().is_empty() {
                problems.push(format!("{}: empty selector", s.name));
            }
            let hashed_class = css.split('.').skip(1).any(|part| {
                let c: String = part.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-').collect();
                c.starts_with("css-") || (c.len() >= 6 && c.chars().all(|ch| ch.is_ascii_alphanumeric()) && c.chars().any(|ch| ch.is_ascii_digit()))
            });
            if hashed_class {
                problems.push(format!("{}: `{css}` relies on a hashed class name", s.name));
            }
            if css.matches('[').count() != css.matches(']').count() || css.matches('(').count() != css.matches(')').count() {
                problems.push(format!("{}: `{css}` has unbalanced brackets", s.name));
            }
        }
    }
    problems
}

fn list(s: &Selector) -> String {
    json!(s.all()).to_string()
}

/// Shared JS prelude: helpers every script below uses. Read-only.
fn prelude() -> String {
    format!(
        r#"
const $all = (sels, root) => {{ for (const s of sels) {{ const el = (root || document).querySelector(s); if (el) return el; }} return null; }};
const TW = {tweet};
const articleFor = (id) => [...document.querySelectorAll(TW[0])].find(a => a.querySelector('a[href*="/status/' + id + '"]')) || null;
const visible = (el) => {{ const r = el.getBoundingClientRect(); return r.width > 0 && r.height > 0; }};
const pageState = () => {{
  const p = location.pathname;
  if ({out}.some(x => p.startsWith(x))) return 'logged_out';
  const ch = [{cf}, {ark}, {txt}];
  if (ch.some(l => l.some(s => document.querySelector(s)))) return 'challenge';
  return 'ok';
}};
"#,
        tweet = list(&sel::TWEET),
        out = json!(LOGGED_OUT_PATHS),
        cf = list(&sel::CHALLENGE_CLOUDFLARE),
        ark = list(&sel::CHALLENGE_ARKOSE),
        txt = list(&sel::CHALLENGE_TEXT_INPUT),
    )
}

/// Reads every tweet card currently rendered. Returns `{state, me, posts[]}`; never interacts with the page.
/// Besides the text it reads the quote card, the thread parent rendered right above a reply, up to two image
/// URLs, whether the text is cut off ("Show more") and whether a sensitive-content wall hides the media.
pub fn extract_feed_js() -> String {
    format!(
        r#"(() => {{
{prelude}
const text = (a, s) => ($all(s, a) || {{}}).innerText || '';
const me = (((($all({acct}) || {{}}).innerText) || '').match(/@([A-Za-z0-9_]{{1,15}})/) || [])[1] || null;
const REPLYING = /(?:Replying to|Antwort an|En respuesta a|En réponse à|In risposta a)\s+@([A-Za-z0-9_]{{1,15}})/i;
const SENSITIVE = /sensib|sensitiv|empfindlich/i;
const posts = [];
let prev = null;
for (const a of document.querySelectorAll({tweet}[0])) {{
  const link = a.querySelector('a[href*="/status/"] time')?.closest('a');
  const m = (link?.getAttribute('href') || '').match(/^\/([^/]+)\/status\/(\d+)/);
  const quote = $all({quote}, a);
  const bodies = [];
  for (const s of {ttext}) for (const el of a.querySelectorAll(s)) if (!bodies.includes(el)) bodies.push(el);
  const body = bodies.find(el => !quote || !quote.contains(el)) || null;
  const quoted = quote ? ((bodies.find(el => quote.contains(el)) || quote).innerText || '') : null;
  const replying = (a.innerText.match(REPLYING) || [])[1] || null;
  const handle = m ? m[1] : null;
  const parent = replying && prev && prev.handle && prev.handle.toLowerCase() === replying.toLowerCase() ? prev.text : null;
  posts.push({{
    id: m ? m[2] : null,
    handle,
    author: text(a, {author}).split('\n')[0],
    text: body ? body.innerText : '',
    lang: body ? body.getAttribute('lang') : null,
    promoted: !!$all({promo}, a),
    reply_restricted: !!$all({restricted}, a),
    has_media: !!$all({media}, a),
    time: a.querySelector('time')?.getAttribute('datetime') || null,
    metrics_label: ($all({metrics}, a) || {{ getAttribute: () => null }}).getAttribute('aria-label'),
    quoted,
    parent,
    replying_to: replying,
    image_urls: [...a.querySelectorAll({image}[0])].filter(i => !quote || !quote.contains(i)).map(i => i.src).filter(u => /^https:\/\/pbs\.twimg\.com\//.test(u)).slice(0, 2),
    truncated: !!$all({more}, a),
    sensitive: [...a.querySelectorAll('[role="button"]')].some(b => SENSITIVE.test(b.innerText || ''))
  }});
  prev = {{ handle, text: body ? body.innerText : '' }};
}}
return {{ state: pageState(), me, url: location.href, posts }};
}})()"#,
        prelude = prelude(),
        acct = list(&sel::ACCOUNT_SWITCHER),
        tweet = list(&sel::TWEET),
        ttext = list(&sel::TWEET_TEXT),
        author = list(&sel::AUTHOR),
        promo = list(&sel::PROMOTED),
        restricted = list(&sel::REPLY_RESTRICTED),
        media = list(&sel::MEDIA),
        metrics = list(&sel::METRICS_GROUP),
        quote = list(&sel::QUOTE_CARD),
        image = list(&sel::IMAGE),
        more = list(&sel::SHOW_MORE),
    )
}

/// Image hosts a thumbnail may be fetched from.
pub fn is_image_url(u: &str) -> bool {
    u.starts_with("https://pbs.twimg.com/") && u.len() < 600 && !u.contains(char::is_whitespace)
}

/// Downloads one post image inside the page (same session, no credentials sent) and returns a small
/// base64 JPEG, or `null`. This is the only script that fetches; it is never part of the read-only set.
pub fn thumbnail_js(url: &str) -> String {
    format!(
        r#"(async () => {{
try {{
  const r = await fetch({url}, {{ credentials: 'omit' }});
  if (!r.ok) return null;
  const b = await r.blob();
  if (b.size > 6000000) return null;
  const bmp = await createImageBitmap(b);
  const k = Math.min(1, 512 / Math.max(bmp.width, bmp.height));
  const c = document.createElement('canvas');
  c.width = Math.max(1, Math.round(bmp.width * k));
  c.height = Math.max(1, Math.round(bmp.height * k));
  c.getContext('2d').drawImage(bmp, 0, 0, c.width, c.height);
  return c.toDataURL('image/jpeg', 0.6).split(',')[1] || null;
}} catch (e) {{ return null; }}
}})()"#,
        url = json!(url)
    )
}

/// Installs (once per page load) a listener that remembers when the user last touched the page and returns
/// the milliseconds since then (SCT-14). While `busy` is set the app's own input is ignored.
pub fn user_idle_js() -> String {
    r#"(() => {
const k = '__sghActivity';
if (!window[k]) {
  const st = { last: 0, busy: false };
  window[k] = st;
  for (const t of ['mousemove', 'mousedown', 'keydown', 'wheel', 'touchstart']) {
    window.addEventListener(t, (e) => { if (e.isTrusted && !st.busy) st.last = Date.now(); }, { capture: true, passive: true });
  }
}
return window[k].last === 0 ? 1000000000 : Date.now() - window[k].last;
})()"#
        .to_string()
}

/// Marks the app's own scrolling so it is not mistaken for the user.
pub fn set_busy_js(busy: bool) -> String {
    format!("(() => {{ const s = window['__sghActivity']; if (s) {{ s.busy = {busy}; if ({busy}) s.last = s.last; }} return true; }})()")
}

/// Reads the target post's text and the live state of its four no-text actions. Read-only.
/// `follow` is `"unknown"` when no inline follow control is rendered.
pub fn probe_js(post_id: &str) -> String {
    format!(
        r#"(() => {{
{prelude}
const state = pageState();
const a = articleFor({id});
if (!a) return {{ page: state, found: false, text: '', like: 'missing', bookmark: 'missing', repost: 'missing', follow: 'unknown', url: location.href }};
const pair = (done, todo) => $all(done, a) ? 'done' : ($all(todo, a) ? 'todo' : 'missing');
const body = $all({ttext}, a);
return {{
  page: state, found: true, url: location.href,
  text: body ? body.innerText : '',
  like: pair({unlike}, {like}),
  bookmark: pair({unbookmark}, {bookmark}),
  repost: pair({unretweet}, {retweet}),
  follow: $all({unfollow}, a) ? 'done' : ($all({follow}, a) ? 'todo' : 'unknown')
}};
}})()"#,
        prelude = prelude(),
        id = json!(post_id),
        ttext = list(&sel::TWEET_TEXT),
        unlike = list(&sel::UNLIKE),
        like = list(&sel::LIKE),
        unbookmark = list(&sel::UNBOOKMARK),
        bookmark = list(&sel::BOOKMARK),
        unretweet = list(&sel::UNRETWEET),
        retweet = list(&sel::RETWEET),
        unfollow = list(&sel::UNFOLLOW),
        follow = list(&sel::FOLLOW),
    )
}

/// Scrolls the first visible match into view and returns its centre `{x, y}` (or `null`). `post_id` limits
/// the search to that post's card; `text_contains` keeps only elements whose text includes it.
pub fn rect_js(post_id: Option<&str>, selectors: &Selector, text_contains: Option<&str>) -> String {
    format!(
        r#"(() => {{
{prelude}
const root = {scope};
if (!root) return null;
const want = {needle};
for (const s of {sels}) {{
  for (const el of root.querySelectorAll(s)) {{
    if (!visible(el)) continue;
    if (want && !(el.textContent || '').toLowerCase().includes(want)) continue;
    el.scrollIntoView({{ block: 'center', inline: 'center' }});
    const r = el.getBoundingClientRect();
    return {{ x: r.left + r.width / 2, y: r.top + r.height / 2 }};
  }}
}}
return null;
}})()"#,
        prelude = prelude(),
        scope = match post_id {
            Some(id) => format!("articleFor({})", json!(id)),
            None => "document".to_string(),
        },
        needle = match text_contains {
            Some(t) => json!(t.to_lowercase()).to_string(),
            None => "null".to_string(),
        },
        sels = list(selectors),
    )
}

/// Text currently in the composer, for the read-back check before posting.
pub fn composer_text_js() -> String {
    format!(
        "(() => {{ {prelude} const c = $all({c}); return c ? c.innerText : null; }})()",
        prelude = prelude(),
        c = list(&sel::COMPOSER)
    )
}

/// Whether an enabled submit button is present.
pub fn submit_ready_js() -> String {
    format!(
        "(() => {{ {prelude} for (const s of {s}) {{ for (const b of document.querySelectorAll(s)) {{ if (visible(b) && b.getAttribute('aria-disabled') !== 'true' && !b.disabled) return true; }} }} return false; }})()",
        prelude = prelude(),
        s = list(&sel::SUBMIT)
    )
}

/// Rect of the first enabled submit button.
pub fn submit_rect_js() -> String {
    format!(
        r#"(() => {{ {prelude}
for (const s of {s}) {{ for (const b of document.querySelectorAll(s)) {{
  if (visible(b) && b.getAttribute('aria-disabled') !== 'true' && !b.disabled) {{ const r = b.getBoundingClientRect(); return {{ x: r.left + r.width / 2, y: r.top + r.height / 2 }}; }}
}} }}
return null; }})()"#,
        prelude = prelude(),
        s = list(&sel::SUBMIT)
    )
}

/// After posting: the id of a card by `me` whose text starts with `snippet`, or a status link in the toast.
pub fn find_published_js(me: Option<&str>, snippet: &str) -> String {
    format!(
        r#"(() => {{ {prelude}
const want = {snip};
const me = {me};
if (me) {{
  for (const a of document.querySelectorAll({tweet}[0])) {{
    const link = a.querySelector('a[href*="/status/"] time')?.closest('a');
    const m = (link?.getAttribute('href') || '').match(/^\/([^/]+)\/status\/(\d+)/);
    const body = $all({ttext}, a);
    if (m && m[1].toLowerCase() === me.toLowerCase() && body && body.innerText.replace(/\s+/g, ' ').trim().startsWith(want)) return m[2];
  }}
}}
const t = $all({toast});
const l = t ? t.querySelector('a[href*="/status/"]') : null;
const m2 = l ? (l.getAttribute('href') || '').match(/status\/(\d+)/) : null;
return m2 ? m2[1] : null; }})()"#,
        prelude = prelude(),
        snip = json!(snippet),
        me = me.map(|m| json!(m).to_string()).unwrap_or_else(|| "null".into()),
        tweet = list(&sel::TWEET),
        ttext = list(&sel::TWEET_TEXT),
        toast = list(&sel::TOAST),
    )
}

/// Texts of the open menu items (used to read "Follow @x" / "Unfollow @x" from the caret menu).
pub fn menu_texts_js() -> String {
    format!(
        "(() => {{ {prelude} return [...document.querySelectorAll({m}[0])].filter(visible).map(e => (e.innerText || '').trim()); }})()",
        prelude = prelude(),
        m = list(&sel::MENU_ITEM)
    )
}

/// True once the composer is gone (post submitted or dialog closed).
pub fn composer_gone_js() -> String {
    format!("(() => {{ {prelude} return !$all({c}); }})()", prelude = prelude(), c = list(&sel::COMPOSER))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_passes_its_own_self_check() {
        assert_eq!(self_check(), Vec::<String>::new());
    }

    #[test]
    fn self_check_catches_bad_selectors() {
        const BAD: Selector = Selector { name: "BAD", primary: "div.css-1dbjc4n", fallbacks: &["[data-testid=\"x\""] };
        let mut found = vec![];
        for css in BAD.all() {
            if css.matches('[').count() != css.matches(']').count() {
                found.push("unbalanced");
            }
            if css.contains(".css-") {
                found.push("hashed");
            }
        }
        assert_eq!(found, vec!["hashed", "unbalanced"]);
    }

    #[test]
    fn read_scripts_never_mutate_or_click() {
        for js in [extract_feed_js(), probe_js("1"), composer_text_js(), menu_texts_js(), composer_gone_js()] {
            for banned in [".click(", ".submit(", "dispatchEvent", "fetch(", "XMLHttpRequest", "innerHTML =", "textContent =", ".value ="] {
                assert!(!js.contains(banned), "script contains {banned}");
            }
        }
    }

    #[test]
    fn feed_script_reads_context_and_flags() {
        let js = extract_feed_js();
        for needle in ["quoteTweet", "show-more-link", "quoted", "parent", "image_urls", "truncated", "sensitive", "Replying to"] {
            assert!(js.contains(needle), "feed script lacks {needle}");
        }
    }

    #[test]
    fn thumbnail_urls_are_restricted_and_embedded_as_json() {
        assert!(is_image_url("https://pbs.twimg.com/media/abc.jpg?name=small"));
        assert!(!is_image_url("https://evil.example/x.jpg"));
        assert!(!is_image_url("https://pbs.twimg.com/a b"));
        let js = thumbnail_js("https://pbs.twimg.com/\"x");
        assert!(js.contains("\\\"x"));
    }

    #[test]
    fn idle_tracker_ignores_synthetic_and_busy_input() {
        let js = user_idle_js();
        assert!(js.contains("isTrusted") && js.contains("busy"));
        assert!(!js.contains("dispatchEvent"));
        assert!(set_busy_js(true).contains("busy = true"));
    }

    #[test]
    fn post_ids_are_embedded_as_json_strings() {
        let js = probe_js("1\"; alert(1); \"");
        assert!(js.contains("\\\"; alert(1);"), "id must be escaped, got a raw quote");
    }

    #[test]
    fn like_and_unlike_use_distinct_testids() {
        assert_ne!(sel::LIKE.primary, sel::UNLIKE.primary);
        assert_ne!(sel::RETWEET.primary, sel::UNRETWEET.primary);
        assert_ne!(sel::BOOKMARK.primary, sel::UNBOOKMARK.primary);
    }
}

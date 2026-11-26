//! The few web pages the account server serves on `mnlab.cc`: sign-in results, invite links and the
//! admin page. The rest of mnlab.cc (home, privacy, changelog) is the static site in `site/`, served
//! by Caddy. Plain HTML, grey only (the app's colour rules), English only for now.

use super::store::Stats;

pub const DOWNLOAD: &str = "https://github.com/chicodev11/Monolab/releases/latest";

pub fn esc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for letter in text.chars() {
        match letter {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }
    out
}

fn page(title: &str, body: &str) -> String {
    layout(title, "", body)
}

/// The shared look of every page here, matching https://mnlab.cc: greys only, the system's dark or
/// light, a small wordmark and the site's footer links. Self-contained: no scripts, fonts or images
/// from anywhere. `class` widens `main` for the admin page.
fn layout(title: &str, class: &str, body: &str) -> String {
    format!(
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<meta name="robots" content="noindex"><meta name="color-scheme" content="dark light"><title>{title} · Monolab</title><style>
:root{{--bg:#f6f6f6;--panel:#fff;--text:#141414;--text-2:#3a3a3a;--muted:#6a6a6a;--line:#e2e2e2;--raise:#efefef;--primary:#141414;--primary-ink:#fff}}
@media (prefers-color-scheme:dark){{:root{{--bg:#111;--panel:#1a1a1a;--text:#f5f5f5;--text-2:#d0d0d0;--muted:#a3a3a3;--line:#2e2e2e;--raise:#272727;--primary:#f5f5f5;--primary-ink:#0f0f0f}}}}
*{{box-sizing:border-box}}
body{{margin:0;min-height:100vh;display:flex;flex-direction:column;background:var(--bg);color:var(--text);
font:15px/1.6 "Inter Variable",Inter,ui-sans-serif,system-ui,-apple-system,"Segoe UI",Roboto,sans-serif;-webkit-font-smoothing:antialiased;overflow-wrap:break-word}}
a{{color:inherit}}
header,footer{{width:100%;max-width:1160px;margin:0 auto;padding:0 28px}}
header{{height:64px;display:flex;align-items:center}}
.mark{{font-size:15px;font-weight:650;text-decoration:none;letter-spacing:-.01em}}
.wrap{{flex:1 0 auto;display:flex;align-items:center;justify-content:center;padding:40px 20px 64px}}
main{{width:100%;max-width:460px;background:var(--panel);border:1px solid var(--line);border-radius:18px;padding:36px}}
main.wide{{max-width:1040px}}
h1{{font-size:24px;line-height:1.25;font-weight:650;letter-spacing:-.015em;margin:0 0 10px}}
h2{{font-size:15px;font-weight:600;margin:0 0 12px}}
p{{margin:0 0 12px;color:var(--text-2)}}.muted{{color:var(--muted);font-size:13px}}
dl{{display:grid;grid-template-columns:auto 1fr;gap:10px 20px;margin:24px 0;padding:20px 0;border-top:1px solid var(--line);border-bottom:1px solid var(--line)}}
dt{{color:var(--muted)}}dd{{margin:0}}
.chip{{display:inline-block;background:var(--raise);border-radius:999px;padding:1px 10px;font-size:13px}}
.actions{{display:flex;flex-wrap:wrap;gap:10px;margin-top:28px}}
.button{{display:inline-flex;align-items:center;min-height:40px;padding:0 18px;border-radius:10px;text-decoration:none;font:inherit;font-weight:550;
border:1px solid var(--line);background:transparent;color:var(--text);cursor:pointer;transition:background-color 140ms ease,border-color 140ms ease}}
.button:hover{{background:var(--raise)}}
.button.primary{{background:var(--primary);color:var(--primary-ink);border-color:var(--primary)}}
.button.primary:hover{{opacity:.9}}
.link{{border:0;background:none;padding:0;font:inherit;color:var(--muted);text-decoration:underline;cursor:pointer}}
:focus-visible{{outline:2px solid var(--text);outline-offset:2px}}
.top{{display:flex;flex-wrap:wrap;align-items:baseline;justify-content:space-between;gap:12px;margin-bottom:28px}}
.top form{{margin:0}}
.tiles{{display:grid;grid-template-columns:repeat(auto-fit,minmax(150px,1fr));gap:12px;margin:0 0 32px}}
.tile{{border:1px solid var(--line);border-radius:14px;padding:16px 18px}}
.tile b{{display:block;font-size:26px;font-weight:650;line-height:1.2;font-variant-numeric:tabular-nums}}
.tile span{{color:var(--muted);font-size:13px}}
.tables{{display:grid;grid-template-columns:repeat(auto-fit,minmax(220px,1fr));gap:28px}}
table{{width:100%;border-collapse:collapse;font-variant-numeric:tabular-nums}}
th,td{{text-align:left;padding:7px 0;border-bottom:1px solid var(--line)}}
th{{color:var(--muted);font-weight:500;font-size:13px}}td.n,th.n{{text-align:right}}
footer{{display:flex;flex-wrap:wrap;gap:20px;padding-top:24px;padding-bottom:32px;font-size:13px;color:var(--muted)}}
footer a{{text-decoration:none}}footer a:hover{{color:var(--text)}}
@media (max-width:520px){{main{{padding:26px 22px;border-radius:14px}}header,footer{{padding-left:20px;padding-right:20px}}}}
@media (prefers-reduced-motion:reduce){{*{{transition:none!important;animation:none!important}}}}
</style></head><body><header><a class="mark" href="https://mnlab.cc">Monolab</a></header>
<div class="wrap"><main class="{class}">{body}</main></div>
<footer><a href="/privacy">Privacy</a><a href="/changelog">Changelog</a><a href="{DOWNLOAD}">Download</a></footer></body></html>"#
    )
}

pub fn signed_in() -> String {
    page(
        "Done with Discord",
        "<h1>Done with Discord</h1><p>Go back to Monolab. If you're new, pick your user ID there.</p><p class=\"muted\">You can close this tab.</p>",
    )
}

pub fn failed(reason: &str) -> String {
    let english = match reason {
        "denied" => "Sign-in was cancelled on Discord.",
        "expired" => "This sign-in expired. Start again in the app.",
        "disabled" => "This account can't be used.",
        "too_many_devices" => "Too many devices. Remove one you don't use in the app's Monolink settings.",
        _ => "Couldn't sign in. Try again in a moment.",
    };
    page("Couldn't sign in", &format!("<h1>Couldn't sign in</h1><p>{english}</p>"))
}

pub struct InvitePreview<'a> {
    pub code: &'a str,
    pub team: &'a str,
    pub by: &'a str,
    pub role: &'a str,
    pub members: usize,
    pub needs_approval: bool,
}

pub fn invite(preview: &InvitePreview<'_>) -> String {
    let role = match preview.role {
        "admin" => "Admin",
        "developer" => "Developer",
        _ => "Viewer",
    };
    let approval = if preview.needs_approval { "<p class=\"muted\">You join once an admin approves.</p>" } else { "" };
    page(
        &format!("Invite to {}", esc(preview.team)),
        &format!(
            "<h1>You're invited to {team}</h1><p class=\"muted\">A team on Monolab.</p>
<dl><dt>Invited by</dt><dd>@{by}</dd><dt>Role</dt><dd><span class=\"chip\">{role}</span></dd><dt>Members</dt><dd>{members}</dd></dl>{approval}
<div class=\"actions\"><a class=\"button primary\" href=\"monolab://invite/{code}\">Open in Monolab</a>
<a class=\"button\" href=\"{DOWNLOAD}\">Download Monolab</a></div>
<p class=\"muted\" style=\"margin-top:16px\">If the app doesn't open, paste this link into the app's Monolink page.</p>",
            team = esc(preview.team),
            by = esc(preview.by),
            members = preview.members,
            code = esc(preview.code),
        ),
    )
}

pub fn invite_unusable(reason: &str) -> String {
    let english = match reason {
        "expired" => "This invite link has expired.",
        "used_up" => "This invite link has been used up.",
        "revoked" => "This invite link was cancelled.",
        _ => "This invite link doesn't exist.",
    };
    page("Invite link", &format!("<h1>This invite can't be used</h1><p>{english} Ask whoever invited you for a new link.</p>"))
}

pub fn admin_login() -> String {
    page(
        "Admin",
        "<h1>Admin</h1><p>Sign in with Discord to see Monolab's usage numbers.</p>
<div class=\"actions\"><a class=\"button primary\" href=\"/admin/login\">Sign in with Discord</a></div>",
    )
}

pub fn admin_forbidden() -> String {
    page(
        "Not allowed",
        "<h1>Not allowed</h1><p>This account can't open the admin page.</p>
<div class=\"actions\"><form method=\"post\" action=\"/admin/logout\"><button class=\"button\" type=\"submit\">Sign out</button></form></div>",
    )
}

pub struct AdminView<'a> {
    pub handle: &'a str,
    /// Apps connected to `/v2/app` now, and how many of them send no usage counts.
    pub connected: u64,
    pub anonymous: u64,
    /// The release list's add-on version and fork build.
    pub addon: u64,
    pub fork: u64,
    pub stats: &'a Stats,
}

fn tile(value: u64, label: &str) -> String {
    format!("<div class=\"tile\"><b>{value}</b><span>{label}</span></div>")
}

fn table(title: &str, column: &str, rows: &[(String, u64)]) -> String {
    let body: String = if rows.is_empty() {
        "<tr><td class=\"muted\" colspan=\"2\">None yet</td></tr>".into()
    } else {
        rows.iter().map(|(value, count)| format!("<tr><td>{}</td><td class=\"n\">{count}</td></tr>", esc(value))).collect()
    };
    format!(
        "<section><h2>{title}</h2><table><thead><tr><th>{column}</th><th class=\"n\">Installs</th></tr></thead><tbody>{body}</tbody></table></section>"
    )
}

pub fn admin(view: &AdminView<'_>) -> String {
    let stats = view.stats;
    let tiles = [
        tile(view.connected, &format!("Apps connected now ({} without usage counts)", view.anonymous)),
        tile(stats.today, "Users today"),
        tile(stats.week, "Users in 7 days"),
        tile(stats.month, "Users in 30 days"),
        tile(stats.accounts, "Accounts"),
        tile(stats.devices, "Signed-in devices"),
        tile(view.addon, "Add-on in the release list"),
        tile(view.fork, "Fork build in the release list"),
    ]
    .concat();
    let tables = [
        table("App", "Version", &stats.by_app),
        table("Add-on", "Version", &stats.by_addon),
        table("Fork", "Build", &stats.by_fork),
        table("OS", "OS", &stats.by_os),
    ]
    .concat();
    layout(
        "Admin",
        "wide",
        &format!(
            "<div class=\"top\"><h1>Usage</h1><form method=\"post\" action=\"/admin/logout\"><span class=\"muted\">@{handle} · </span><button class=\"link\" type=\"submit\">Sign out</button></form></div>
<div class=\"tiles\">{tiles}</div><div class=\"tables\">{tables}</div>
<p class=\"muted\" style=\"margin:28px 0 0\">Versions and OS count installs seen in the last 30 days ({installs}). Days are UTC.</p>",
            handle = esc(view.handle),
            installs = stats.installs,
        ),
    )
}

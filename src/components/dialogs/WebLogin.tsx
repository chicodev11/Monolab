import { tr } from "../../lib/i18n";
import appIcon from "../../assets/app-icon.png";
import { useEffect, useRef, useState } from "react";
import { onWeb, setWebSession } from "../../lib/bridge";

/** The token from an invite link (`#invite=…`), if this page was opened from one. */
export function inviteToken(hash = typeof window === "undefined" ? "" : window.location.hash): string {
  const found = hash.match(/[#&]invite=([0-9a-f]{64})\b/i);
  return found ? found[1].toLowerCase() : "";
}

function dropInviteFromAddress() {
  try {
    window.history.replaceState(null, "", window.location.pathname + window.location.search);
  } catch {
    window.location.hash = "";
  }
}

async function post(path: string, body: unknown) {
  const answer = await fetch(path, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  const data = await answer.json().catch(() => ({}));
  if (data?.error) throw new Error(data.error);
  return data;
}

export function WebLogin({ onDone }: { onDone: () => void }) {
  const [token] = useState(inviteToken);
  const [invite, setInvite] = useState<{ server: string; role: string; roleName?: string } | null>(null);
  const [inviteGone, setInviteGone] = useState("");
  const [haveAccount, setHaveAccount] = useState(false);
  const [name, setName] = useState("");
  const [password, setPassword] = useState("");
  const [again, setAgain] = useState("");
  const [why, setWhy] = useState("");
  const [busy, setBusy] = useState(false);
  const nameRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (window.innerWidth >= 900) nameRef.current?.focus();
  }, [invite]);

  useEffect(() => {
    if (!token) return;
    post("/invite-info", { token }).then(
      (info) => setInvite({ server: String(info.server ?? ""), role: String(info.role ?? "") }),
      (cause) => setInviteGone(String(cause instanceof Error ? cause.message : cause)),
    );
  }, [token]);

  async function go() {
    if (busy) return;
    setWhy("");
    if (invite && !haveAccount && password !== again) {
      setWhy(tr("The passwords don't match."));
      return;
    }
    setBusy(true);
    try {
      const data = invite ? await post("/accept-invite", { token, name, password }) : await post("/login", { name, password });
      setWebSession(data.session);
      if (invite) dropInviteFromAddress();
      onDone();
    } catch (cause) {
      setWhy(String(cause instanceof Error ? cause.message : cause));
    } finally {
      setBusy(false);
    }
  }

  if (!onWeb) return null;
  const joining = !!invite;
  return (
    <div className="web-login">
      <form
        className="web-login-card"
        onSubmit={(event) => {
          event.preventDefault();
          void go();
        }}
      >
        <span className="brand-mark">
          <img className="brand-img" src={appIcon} alt="" draggable={false} />
        </span>
        <h1>{joining ? tr("Join {0}", [invite.server]) : "Monolab"}</h1>
        {joining ? (
          <p>
            {invite.role === "Viewer"
              ? tr("You're invited to watch this server: console, logs, files and players.")
              : invite.role.startsWith("role:")
                ? tr("You're invited to this server as {0}.", [invite.roleName ?? tr("a member")])
                : tr("You're invited to help run this server: start and stop it, use the console and edit files.")}{" "}
            {haveAccount ? tr("Sign in and it's added to your account.") : tr("Pick a name and password for yourself.")}
          </p>
        ) : (
          <p>{tr("Sign in with an account created in the app.")}</p>
        )}
        {token && inviteGone && (
          <p className="web-login-bad">
            {inviteGone}{" "}
            <button type="button" className="link-button" onClick={() => { dropInviteFromAddress(); setInviteGone(""); }}>
              {tr("Sign in instead")}
            </button>
          </p>
        )}
        <label className="field-block">
          <span className="field-label">{joining && !haveAccount ? tr("Your name") : tr("Account name")}</span>
          <input
            ref={nameRef}
            value={name}
            maxLength={32}
            autoComplete="username"
            autoCapitalize="off"
            onChange={(event) => setName(event.target.value)}
          />
        </label>
        <label className="field-block">
          <span className="field-label">{tr("Password")}</span>
          <input
            type="password"
            value={password}
            autoComplete={joining && !haveAccount ? "new-password" : "current-password"}
            onChange={(event) => setPassword(event.target.value)}
          />
        </label>
        {joining && !haveAccount && (
          <label className="field-block">
            <span className="field-label">{tr("Password again")}</span>
            <input type="password" value={again} autoComplete="new-password" onChange={(event) => setAgain(event.target.value)} />
            <small className="muted">{tr("At least 8 characters.")}</small>
          </label>
        )}
        {why && <p className="web-login-bad">{why}</p>}
        <button className="button primary" type="submit" disabled={busy || (!!token && !invite && !inviteGone)}>
          {busy ? tr("Checking…") : joining ? (haveAccount ? tr("Sign in and join") : tr("Create account and join")) : tr("Sign in")}
        </button>
        {joining && (
          <button type="button" className="link-button web-login-switch" onClick={() => { setHaveAccount(!haveAccount); setWhy(""); }}>
            {haveAccount ? tr("I'm new here") : tr("I already have an account")}
          </button>
        )}
      </form>
    </div>
  );
}

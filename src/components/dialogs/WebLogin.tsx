import { tr } from "../../lib/i18n";
import appIcon from "../../assets/app-icon.png";
import { useEffect, useRef, useState } from "react";
import { onWeb, setWebSession } from "../../lib/bridge";

export function WebLogin({ onDone }: { onDone: () => void }) {
  const [name, setName] = useState("");
  const [password, setPassword] = useState("");
  const [why, setWhy] = useState("");
  const [busy, setBusy] = useState(false);
  const nameRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (window.innerWidth >= 900) nameRef.current?.focus();
  }, []);

  async function go() {
    if (busy) return;
    setBusy(true);
    setWhy("");
    try {
      const answer = await fetch("/login", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ name, password }),
      });
      const data = await answer.json().catch(() => ({}));
      if (data?.error) throw new Error(data.error);
      setWebSession(data.session);
      onDone();
    } catch (cause) {
      setWhy(String(cause instanceof Error ? cause.message : cause));
    } finally {
      setBusy(false);
    }
  }

  if (!onWeb) return null;
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
        <h1>Monolab</h1>
        <p>{tr("Sign in with an account created in the app.")}</p>
        <label className="field-block">
          <span className="field-label">{tr("Account name")}</span>
          <input
            ref={nameRef}
            value={name}
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
            autoComplete="current-password"
            onChange={(event) => setPassword(event.target.value)}
          />
        </label>
        {why && <p className="web-login-bad">{why}</p>}
        <button className="button primary" type="submit" disabled={busy}>
          {busy ? tr("Checking…") : tr("Sign in")}
        </button>
      </form>
    </div>
  );
}

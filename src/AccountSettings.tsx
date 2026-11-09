import { useEffect, useRef, useState } from "react";

import {
  accountErrorText,
  displayName,
  failText,
  focusAfterChange,
  minutesLeft,
  otherSignInsText,
  seatsText,
  serverHost,
  type AccountProblem,
  type AccountState,
  type AccountView,
  type FocusTarget,
} from "./account";
import { writeClipboard } from "./clipboard";
import { useT } from "./i18n";
import { account, useAccount } from "./useAccount";
import { useClock } from "./useClock";

/** Callback refs of the main buttons, by the block they belong to. */
type Register = (target: FocusTarget) => (element: HTMLButtonElement | null) => void;

/**
 * Settings, Account: sign in with the device flow (a code to confirm in the browser), the
 * signed-in person, sign out, and the server address for self-hosting. Everything else (tokens,
 * requests, opening pages) happens in the backend.
 */

export function AccountSettings() {
  const t = useT();
  const { view, unavailable } = useAccount();
  const [problem, setProblem] = useState<AccountProblem | null>(null);
  const [busy, setBusy] = useState(false);
  const buttons = useRef(new Map<FocusTarget, HTMLButtonElement>());
  const previousKind = useRef<AccountState["kind"] | null>(null);
  const pendingFocus = useRef<FocusTarget | null>(null);
  const section = useRef<HTMLElement>(null);
  const kind = view?.state.kind ?? null;

  // A new block replaces the button that had the focus: move it to the new block's main button,
  // once that button is enabled again (it is disabled while the command that changed the state
  // still runs). A change from the backend while the focus is elsewhere (say, in the server
  // field) leaves it there.
  useEffect(() => {
    if (kind === null) return;
    const target = focusAfterChange(previousKind.current, kind);
    previousKind.current = kind;
    const active = document.activeElement;
    const focusLost = active === null || active === document.body;
    if (target !== null && (focusLost || section.current?.contains(active) === true)) {
      pendingFocus.current = target;
    }
    const element =
      pendingFocus.current === null ? undefined : buttons.current.get(pendingFocus.current);
    if (element !== undefined && !element.disabled) {
      element.focus();
      pendingFocus.current = null;
    }
  }, [kind, busy]);

  const register: Register = (target) => (element) => {
    if (element === null) buttons.current.delete(target);
    else buttons.current.set(target, element);
  };

  const run = (action: () => Promise<AccountProblem | null>) => {
    setBusy(true);
    setProblem(null);
    void action()
      .then(setProblem)
      .finally(() => {
        setBusy(false);
      });
  };

  return (
    <>
      <p className="muted settings-intro">{t("account.intro")}</p>
      {view === null ? (
        // Nothing while the first status loads; a text only when it failed.
        unavailable && <p className="muted">{t("account.unavailable")}</p>
      ) : (
        <section ref={section} className="settings-section" aria-labelledby="account-state">
          <h4 id="account-state" className="settings-subtitle">
            {t("account.title")}
          </h4>
          <StateBlock view={view} busy={busy} run={run} register={register} />
          {problem !== null && (
            <p className="muted error-text" role="alert">
              {accountErrorText(problem, t)}
              {problem.detail !== null && problem.kind === "server" && <> {problem.detail}</>}
            </p>
          )}
        </section>
      )}
      {view !== null && <ServerField view={view} />}
    </>
  );
}

function StateBlock({
  view,
  busy,
  run,
  register,
}: {
  view: AccountView;
  busy: boolean;
  run: (action: () => Promise<AccountProblem | null>) => void;
  register: Register;
}) {
  const t = useT();
  const state = view.state;
  const devices = (
    <button
      type="button"
      className="link-button account-link"
      onClick={() => {
        run(() => account.open({ kind: "devices" }));
      }}
    >
      {t("account.devicesPage")}
    </button>
  );
  switch (state.kind) {
    case "idle":
      return (
        <>
          {state.notice === "sessionEnded" && (
            <p className="muted error-text" role="status">
              {t("account.sessionEnded")}
            </p>
          )}
          {state.notice === "signedOut" && (
            <p className="muted" role="status">
              {t("account.signedOut")}
            </p>
          )}
          {state.notice === "signedOutLocally" && (
            <p className="muted" role="status">
              {t("account.signedOutLocally")} {devices}
            </p>
          )}
          {otherSignInsText(view, t) !== null && (
            <p className="muted">{otherSignInsText(view, t)}</p>
          )}
          <div className="settings-row">
            <button
              ref={register("signIn")}
              type="button"
              className="primary-button"
              disabled={busy}
              onClick={() => {
                run(account.signIn);
              }}
            >
              {t("account.signIn")}
            </button>
            {!view.official && (
              <span className="muted">
                {t("account.serverName", { host: serverHost(view.baseUrl) })}
              </span>
            )}
          </div>
        </>
      );
    case "waitingForUser":
      return <WaitingBlock state={state} run={run} register={register} />;
    case "signedIn":
      return (
        <>
          {state.user === null ? (
            <p>{t("account.signedIn")}</p>
          ) : (
            <div className="account-person">
              <span className="account-name">
                {t("account.signedInAs", { name: displayName(state.user) })}
              </span>
              {displayName(state.user) !== state.user.email && (
                <span className="muted">{state.user.email}</span>
              )}
              {seatsText(state.user, t) !== null && (
                <span className="muted">
                  {t("account.ownSeats", { seats: seatsText(state.user, t) ?? "" })}
                </span>
              )}
            </div>
          )}
          <div className="settings-row">
            <button
              ref={register("signOut")}
              type="button"
              className="secondary-button"
              disabled={busy}
              onClick={() => {
                run(account.signOut);
              }}
            >
              {t("account.signOut")}
            </button>
          </div>
          <p className="muted settings-help account-help">
            {t("account.signOutHelp")} {devices}
          </p>
        </>
      );
    case "failed":
      return (
        <>
          <p className="error-text" role="alert">
            {failText(state.reason, t)}
            {state.reason.detail !== null && (
              <span className="muted account-detail"> {state.reason.detail}</span>
            )}
          </p>
          <div className="settings-row">
            <button
              ref={register("tryAgain")}
              type="button"
              className="primary-button"
              disabled={busy}
              onClick={() => {
                run(account.signIn);
              }}
            >
              {t("account.tryAgain")}
            </button>
            <button
              type="button"
              className="secondary-button"
              disabled={busy}
              onClick={() => {
                run(account.cancelSignIn);
              }}
            >
              {t("common.cancel")}
            </button>
          </div>
        </>
      );
  }
}

/** The code to confirm, large, with copy, open again and cancel. */
function WaitingBlock({
  state,
  run,
  register,
}: {
  state: Extract<AccountState, { kind: "waitingForUser" }>;
  run: (action: () => Promise<AccountProblem | null>) => void;
  register: Register;
}) {
  const t = useT();
  const now = useClock(15_000);
  const [copied, setCopied] = useState(false);

  return (
    <div className="account-waiting">
      <p id="account-code-label">{t("account.codeIntro")}</p>
      <output className="account-code" aria-labelledby="account-code-label">
        {state.userCode}
      </output>
      <p className="muted account-manual">
        {t("account.codeManual", { page: state.verificationUri })}
      </p>
      <div className="settings-row">
        <button
          ref={register("copyCode")}
          type="button"
          className="secondary-button"
          onClick={() => {
            writeClipboard(state.userCode)
              .then(() => {
                setCopied(true);
              })
              .catch(() => {
                setCopied(false);
              });
          }}
        >
          {copied ? t("account.copied") : t("account.copyCode")}
        </button>
        <button
          type="button"
          className="secondary-button"
          onClick={() => {
            run(() => account.open({ kind: "verification" }));
          }}
        >
          {t("account.openAgain")}
        </button>
        <button
          type="button"
          className="link-button"
          onClick={() => {
            run(account.cancelSignIn);
          }}
        >
          {t("common.cancel")}
        </button>
      </div>
      <p className="muted" role="status">
        {t("account.waiting", { minutes: minutesLeft(state.expiresAt, now) })}
      </p>
    </div>
  );
}

/** Server address for self-hosting, folded away: almost everyone uses the official service. */
function ServerField({ view }: { view: AccountView }) {
  const t = useT();
  const [text, setText] = useState(view.official ? "" : view.baseUrl);
  // The server can change while the dialog is open (here, or from the backend): show the new one.
  const [shownBase, setShownBase] = useState(view.baseUrl);
  if (shownBase !== view.baseUrl) {
    setShownBase(view.baseUrl);
    setText(view.official ? "" : view.baseUrl);
  }
  const [status, setStatus] = useState<{ ok: boolean; text: string } | null>(null);
  const [busy, setBusy] = useState(false);

  const apply = (value: string | null) => {
    setBusy(true);
    setStatus(null);
    void account
      .setBaseUrl(value)
      .then((problem) => {
        if (problem === null) {
          setStatus({ ok: true, text: t("account.serverSaved") });
          if (value === null) setText("");
        } else {
          setStatus({ ok: false, text: accountErrorText(problem, t) });
        }
      })
      .finally(() => {
        setBusy(false);
      });
  };

  return (
    <details className="settings-section account-server" open={!view.official}>
      <summary className="settings-subtitle">{t("account.server")}</summary>
      <p id="account-server-help" className="muted settings-intro">
        {t("account.serverHelp")}
      </p>
      <form
        className="settings-row account-server-row"
        onSubmit={(event) => {
          event.preventDefault();
          apply(text.trim() === "" ? null : text.trim());
        }}
      >
        <label htmlFor="account-server" className="sr-only">
          {t("account.server")}
        </label>
        <input
          id="account-server"
          className="text-input account-server-input"
          type="url"
          inputMode="url"
          spellCheck={false}
          autoComplete="off"
          placeholder="https://hangar.example.com/api/v1"
          value={text}
          aria-describedby="account-server-help account-server-status"
          aria-invalid={status !== null && !status.ok ? true : undefined}
          onChange={(event) => {
            setText(event.currentTarget.value);
          }}
        />
        <button type="submit" className="secondary-button" disabled={busy}>
          {t("account.serverApply")}
        </button>
        {!view.official && (
          <button
            type="button"
            className="link-button"
            disabled={busy}
            onClick={() => {
              apply(null);
            }}
          >
            {t("account.serverOfficial")}
          </button>
        )}
      </form>
      <p
        id="account-server-status"
        className={status !== null && !status.ok ? "muted error-text" : "muted"}
        role="status"
      >
        {status?.text ?? ""}
      </p>
    </details>
  );
}

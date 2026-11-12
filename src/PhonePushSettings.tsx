// Settings group "To your phone" (phonePush.ts): the service, its address or keys, the events and
// a test push. The backend owns the settings and keeps the token and keys in the keychain; the
// form only learns whether each one is stored and never shows a stored value.

import { useEffect, useState } from "react";

import { api } from "./api";
import { useT, type MessageKey } from "./i18n";
import {
  DEFAULT_PUSH_VIEW,
  isNtfyUrl,
  isWeakTopic,
  needsTokenAgain,
  parsePushView,
  PUSH_EVENTS,
  PUSH_PROVIDERS,
  pushErrorKey,
  randomTopic,
  withTopic,
  type PushEvent,
  type PushEvents,
  type PushInput,
  type PushProvider,
  type PushView,
} from "./phonePush";

type SecretField = "ntfyToken" | "pushoverUser" | "pushoverToken";

const PROVIDER_NAMES: Record<PushProvider, MessageKey> = {
  off: "push.off",
  ntfy: "push.ntfy",
  pushover: "push.pushover",
};

const EVENT_NAMES: Record<PushEvent, MessageKey> = {
  waiting: "push.eventWaiting",
  done: "push.eventDone",
  failed: "push.eventFailed",
};

const SECRET_NAMES: Record<SecretField, MessageKey> = {
  ntfyToken: "push.ntfyToken",
  pushoverUser: "push.pushoverUser",
  pushoverToken: "push.pushoverToken",
};

const NO_SECRETS: Record<SecretField, string> = {
  ntfyToken: "",
  pushoverUser: "",
  pushoverToken: "",
};
const NO_REMOVALS: Record<SecretField, boolean> = {
  ntfyToken: false,
  pushoverUser: false,
  pushoverToken: false,
};

type Status = { kind: "saved" | "sent" } | { kind: "error"; text: MessageKey } | null;

export function PhonePushSettings({ disabled }: { disabled: boolean }) {
  const t = useT();
  const [view, setView] = useState<PushView>(DEFAULT_PUSH_VIEW);
  const [provider, setProvider] = useState<PushProvider>("off");
  const [ntfyUrl, setNtfyUrl] = useState("");
  const [includeTitle, setIncludeTitle] = useState(false);
  const [events, setEvents] = useState<PushEvents>(DEFAULT_PUSH_VIEW.events);
  const [secrets, setSecrets] = useState(NO_SECRETS);
  const [removed, setRemoved] = useState(NO_REMOVALS);
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<Status>(null);

  const adopt = (next: PushView) => {
    setView(next);
    setProvider(next.provider);
    setNtfyUrl(next.ntfyUrl);
    setIncludeTitle(next.includeTitle);
    setEvents(next.events);
    setSecrets(NO_SECRETS);
    setRemoved(NO_REMOVALS);
  };

  useEffect(() => {
    let live = true;
    api
      .pushSettings()
      .then((raw) => {
        if (live) adopt(parsePushView(raw));
      })
      // A keychain that cannot be read must not look like "push is off".
      .catch((error: unknown) => {
        if (live) setStatus({ kind: "error", text: pushErrorKey(error) });
      });
    return () => {
      live = false;
    };
  }, []);

  const dirty =
    provider !== view.provider ||
    ntfyUrl.trim() !== view.ntfyUrl ||
    includeTitle !== view.includeTitle ||
    PUSH_EVENTS.some((event) => events[event] !== view.events[event]) ||
    Object.values(secrets).some((value) => value.trim() !== "") ||
    Object.values(removed).some(Boolean);
  const urlInvalid = ntfyUrl.trim() !== "" && !isNtfyUrl(ntfyUrl);
  const tokenTyped = secrets.ntfyToken.trim() !== "";
  // The stored token is bound to its server: another one needs it again (or its removal).
  const tokenAgain =
    provider === "ntfy" && needsTokenAgain(view, ntfyUrl, tokenTyped, removed.ntfyToken);
  const hasToken = tokenTyped || (view.ntfyToken && !removed.ntfyToken && !tokenAgain);
  const weakTopic = provider === "ntfy" && isWeakTopic(ntfyUrl, hasToken);
  const blocked = urlInvalid || tokenAgain || weakTopic;

  const input = (): PushInput => {
    const secret = (field: SecretField) =>
      removed[field] ? "" : secrets[field].trim() === "" ? null : secrets[field];
    return {
      provider,
      ntfyUrl: ntfyUrl.trim(),
      includeTitle,
      events,
      ntfyToken: secret("ntfyToken"),
      pushoverUser: secret("pushoverUser"),
      pushoverToken: secret("pushoverToken"),
    };
  };

  /** Saves; `false` when the backend refused (the status says why). */
  const save = async (): Promise<boolean> => {
    try {
      adopt(parsePushView(await api.pushSave(input())));
      setStatus({ kind: "saved" });
      return true;
    } catch (error) {
      setStatus({ kind: "error", text: pushErrorKey(error) });
      return false;
    }
  };

  // The buttons stay focusable while busy (aria-disabled), so focus is not lost to the page.
  const run = (work: () => Promise<void>) => {
    if (busy) return;
    setBusy(true);
    setStatus(null);
    work()
      .catch(() => undefined)
      .finally(() => {
        setBusy(false);
      });
  };

  const sendTest = async () => {
    // The test goes out with the saved settings: unsaved changes are saved first.
    if (dirty && !(await save())) return;
    try {
      await api.pushTest(t("push.testText"));
      setStatus({ kind: "sent" });
    } catch (error) {
      setStatus({ kind: "error", text: pushErrorKey(error) });
    }
  };

  const secretField = (field: SecretField, stored: boolean) => {
    const id = `push-${field}`;
    const kept = stored && !removed[field];
    return (
      <div className="field push-secret">
        <label htmlFor={id}>{t(SECRET_NAMES[field])}</label>
        <div className="push-secret-row">
          <input
            id={id}
            type="password"
            className="text-input"
            autoComplete="off"
            spellCheck={false}
            value={secrets[field]}
            aria-describedby={kept ? `${id}-help` : undefined}
            onChange={(event) => {
              const value = event.currentTarget.value;
              setSecrets((current) => ({ ...current, [field]: value }));
              if (value !== "") setRemoved((current) => ({ ...current, [field]: false }));
            }}
          />
          {kept && (
            <button
              type="button"
              className="link-button"
              onClick={() => {
                setRemoved((current) => ({ ...current, [field]: true }));
                setSecrets((current) => ({ ...current, [field]: "" }));
              }}
            >
              {t("push.secretRemove")}
            </button>
          )}
        </div>
        {kept && (
          <p id={`${id}-help`} className="muted settings-help">
            {t("push.secretStored")}
          </p>
        )}
      </div>
    );
  };

  return (
    <fieldset className="settings-group push-settings" disabled={disabled}>
      <legend className="settings-subhead">{t("push.heading")}</legend>
      <p className="muted settings-help">{t("push.help")}</p>

      <div className="field">
        <span id="push-provider">{t("push.provider")}</span>
        <div className="segmented" role="radiogroup" aria-labelledby="push-provider">
          {PUSH_PROVIDERS.map((option) => (
            <label key={option} className="segment">
              <input
                type="radio"
                name="push-provider"
                value={option}
                checked={provider === option}
                onChange={() => {
                  setProvider(option);
                  setStatus(null);
                }}
              />
              <span>{t(PROVIDER_NAMES[option])}</span>
            </label>
          ))}
        </div>
      </div>

      {provider === "ntfy" && (
        <>
          <div className="field">
            <label htmlFor="push-ntfy-url">{t("push.ntfyUrl")}</label>
            <input
              id="push-ntfy-url"
              type="url"
              className="text-input push-url"
              spellCheck={false}
              autoComplete="off"
              placeholder={t("push.ntfyUrlPlaceholder")}
              value={ntfyUrl}
              aria-invalid={urlInvalid}
              aria-describedby="push-ntfy-url-help"
              onChange={(event) => {
                setNtfyUrl(event.currentTarget.value);
              }}
            />
            <p
              id="push-ntfy-url-help"
              className={urlInvalid || weakTopic ? "muted error-text" : "muted"}
            >
              {t(
                urlInvalid
                  ? "push.error.invalidUrl"
                  : weakTopic
                    ? "push.error.weakTopic"
                    : "push.ntfyUrlHelp",
              )}
            </p>
            <button
              type="button"
              className="link-button"
              onClick={() => {
                setNtfyUrl(withTopic(ntfyUrl, randomTopic()));
              }}
            >
              {t("push.randomTopic")}
            </button>
          </div>
          {secretField("ntfyToken", view.ntfyToken)}
          {tokenAgain && (
            <p className="muted error-text push-token-again" role="status">
              {t("push.tokenAgain")}
            </p>
          )}
        </>
      )}

      {provider === "pushover" && (
        <>
          {secretField("pushoverUser", view.pushoverUser)}
          {secretField("pushoverToken", view.pushoverToken)}
        </>
      )}

      {provider !== "off" && (
        <div className="field">
          <label className="check">
            <input
              type="checkbox"
              checked={includeTitle}
              aria-describedby="push-include-title-help"
              onChange={(event) => {
                setIncludeTitle(event.currentTarget.checked);
              }}
            />
            <span>{t("push.includeTitle")}</span>
          </label>
          <p id="push-include-title-help" className="muted settings-help">
            {t("push.includeTitleHelp")}
          </p>
        </div>
      )}

      {provider !== "off" && (
        <div className="field">
          <span id="push-events">{t("push.events")}</span>
          <div className="push-events" role="group" aria-labelledby="push-events">
            {PUSH_EVENTS.map((event) => (
              <label key={event} className="check">
                <input
                  type="checkbox"
                  checked={events[event]}
                  onChange={(change) => {
                    const checked = change.currentTarget.checked;
                    setEvents((current) => ({ ...current, [event]: checked }));
                  }}
                />
                <span>{t(EVENT_NAMES[event])}</span>
              </label>
            ))}
          </div>
        </div>
      )}

      <div className="push-actions">
        <button
          type="button"
          className="secondary-button"
          disabled={!dirty || blocked}
          aria-disabled={busy || undefined}
          onClick={() => {
            run(async () => {
              await save();
            });
          }}
        >
          {t("push.save")}
        </button>
        <button
          type="button"
          className="secondary-button"
          disabled={provider === "off" || blocked}
          aria-disabled={busy || undefined}
          onClick={() => {
            run(sendTest);
          }}
        >
          {t("push.test")}
        </button>
        {/* Always mounted: a live region that appears together with its text is often not read. */}
        <span className={status?.kind === "error" ? "muted error-text" : "muted"} role="status">
          {status === null
            ? ""
            : status.kind === "error"
              ? t(status.text)
              : t(status.kind === "saved" ? "push.saved" : "push.testSent")}
        </span>
      </div>
    </fieldset>
  );
}

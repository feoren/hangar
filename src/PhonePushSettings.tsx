// Settings group "To your phone" (phonePush.ts): the service, its address or keys, the events and
// a test push. The backend owns the settings and keeps the token and keys in the keychain; the
// form only learns whether each one is stored and never shows a stored value.
//
// Unlike the switches elsewhere, this group saves with a button: an address and its token, or the
// two Pushover keys, only make sense together, the backend refuses an incomplete or weak setup, and
// every save writes the keychain. Saving on each keystroke would store half-typed secrets and
// report errors while the user is still typing.

import { useEffect, useState } from "react";

import { api } from "./api";
import { useT, type MessageKey } from "./i18n";
import { SegmentedField, SettingRow, SettingsGroup, SwitchRow } from "./SettingsParts";
import {
  DEFAULT_PUSH_VIEW,
  isNtfyUrl,
  isWeakTopic,
  keepsStoredUrl,
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

export function PhonePushSettings() {
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
  const keptUrl = keepsStoredUrl(view, ntfyUrl);
  const urlInvalid = !keptUrl && ntfyUrl.trim() !== "" && !isNtfyUrl(ntfyUrl);
  const tokenTyped = secrets.ntfyToken.trim() !== "";
  // The stored token is bound to its server: another one needs it again (or its removal).
  const tokenAgain =
    provider === "ntfy" && needsTokenAgain(view, ntfyUrl, tokenTyped, removed.ntfyToken);
  const hasToken = tokenTyped || (view.ntfyToken && !removed.ntfyToken && !tokenAgain);
  const weakTopic = provider === "ntfy" && !keptUrl && isWeakTopic(ntfyUrl, hasToken);
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

  const secretRow = (field: SecretField, stored: boolean) => {
    const kept = stored && !removed[field];
    // The token row also says when a new server needs the token again.
    const again = field === "ntfyToken" && tokenAgain;
    return (
      <SettingRow
        key={field}
        label={t(SECRET_NAMES[field])}
        help={again ? t("push.tokenAgain") : kept ? t("push.secretStored") : undefined}
        helpIsError={again}
        helpIsStatus={field === "ntfyToken"}
        control={({ labelId, helpId }) => (
          <span className="push-field">
            <input
              id={`push-${field}`}
              type="password"
              className="text-input push-input"
              autoComplete="off"
              spellCheck={false}
              value={secrets[field]}
              aria-labelledby={labelId}
              aria-describedby={helpId}
              aria-invalid={again || undefined}
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
          </span>
        )}
      />
    );
  };

  // A success is old news once the form changes again; an error stays until the next attempt.
  const shown = status?.kind === "error" || !dirty ? status : null;
  const statusText =
    shown === null
      ? dirty
        ? t("push.unsaved")
        : ""
      : shown.kind === "error"
        ? t(shown.text)
        : t(shown.kind === "saved" ? "push.saved" : "push.testSent");

  return (
    <SettingsGroup title={t("push.heading")} description={t("push.help")}>
      <SegmentedField
        label={t("push.provider")}
        value={provider}
        options={PUSH_PROVIDERS.map((option) => ({
          value: option,
          label: t(PROVIDER_NAMES[option]),
        }))}
        onChange={(option) => {
          setProvider(option);
          setStatus(null);
        }}
      />

      {provider === "ntfy" && (
        <>
          <SettingRow
            label={t("push.ntfyUrl")}
            help={t(
              urlInvalid
                ? "push.error.invalidUrl"
                : weakTopic
                  ? "push.error.weakTopic"
                  : keptUrl
                    ? "push.ntfyUrlStored"
                    : "push.ntfyUrlHelp",
            )}
            helpIsError={urlInvalid || weakTopic}
            control={({ labelId, helpId }) => (
              <span className="push-field push-url-field">
                <input
                  id="push-ntfy-url"
                  type="url"
                  className="text-input push-input"
                  spellCheck={false}
                  autoComplete="off"
                  placeholder={t("push.ntfyUrlPlaceholder")}
                  value={ntfyUrl}
                  aria-labelledby={labelId}
                  aria-describedby={helpId}
                  aria-invalid={urlInvalid || weakTopic || undefined}
                  onChange={(event) => {
                    setNtfyUrl(event.currentTarget.value);
                  }}
                />
                <button
                  type="button"
                  className="link-button"
                  onClick={() => {
                    setNtfyUrl(withTopic(ntfyUrl, randomTopic()));
                  }}
                >
                  {t("push.randomTopic")}
                </button>
              </span>
            )}
          />
          {secretRow("ntfyToken", view.ntfyToken)}
        </>
      )}

      {provider === "pushover" && (
        <>
          {secretRow("pushoverUser", view.pushoverUser)}
          {secretRow("pushoverToken", view.pushoverToken)}
        </>
      )}

      {provider !== "off" && (
        <>
          {PUSH_EVENTS.map((event) => (
            <SwitchRow
              key={event}
              label={t(EVENT_NAMES[event])}
              checked={events[event]}
              onChange={(checked) => {
                setEvents((current) => ({ ...current, [event]: checked }));
              }}
            />
          ))}
          <SwitchRow
            label={t("push.includeTitle")}
            help={t("push.includeTitleHelp")}
            checked={includeTitle}
            onChange={setIncludeTitle}
          />
        </>
      )}

      <div className="setting">
        <div className="setting-line">
          <span className="setting-text">
            {/* Always mounted: a live region that appears together with its text is often not
                read. */}
            <span
              className={status?.kind === "error" ? "setting-help error-text" : "setting-help"}
              role="status"
            >
              {statusText}
            </span>
          </span>
          <span className="setting-control">
            {provider !== "off" && (
              <button
                type="button"
                className="secondary-button"
                disabled={blocked}
                aria-disabled={busy || undefined}
                onClick={() => {
                  run(sendTest);
                }}
              >
                {t("push.test")}
              </button>
            )}
            {/* With the push off and nothing changed there is nothing to save. */}
            {(provider !== "off" || dirty) && (
              <button
                type="button"
                className="primary-button"
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
            )}
          </span>
        </div>
      </div>
    </SettingsGroup>
  );
}

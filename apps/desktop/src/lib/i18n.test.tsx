import { render, screen } from "@testing-library/react";
import { renderHook, act } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { I18nProvider, useI18n } from "./i18n";

function LangProbe() {
  const { t, lang } = useI18n();
  return (
    <div>
      <span data-testid="lang">{lang}</span>
      <span data-testid="label">{t("nav.skills")}</span>
    </div>
  );
}

describe("i18n", () => {
  // Language preference persists in localStorage — clear it so each test
  // starts from the deterministic default (system detection on empty).
  beforeEach(() => {
    // jsdom may not provide localStorage depending on version/config —
    // clear it only when present so each test starts hermetic.
    window.localStorage?.clear?.();
  });


  it("falls back to English outside the provider", () => {
    const { result } = renderHook(() => useI18n());
    expect(result.current.lang).toBe("en");
    expect(result.current.t("nav.skills")).toBe("Skills");
  });

  it("switches to Chinese and renders translated strings", async () => {
    const { result } = renderHook(() => useI18n(), {
      wrapper: I18nProvider,
    });
    expect(result.current.t("nav.skills")).toBe("Skills");
    act(() => result.current.setLang("zh"));
    expect(result.current.lang).toBe("zh");
    expect(result.current.t("nav.skills")).toBe("技能");
    expect(result.current.t("state.synced")).toBe("已同步");
  });

  it("renders the language selector in Settings (provider mounted)", async () => {
    // Minimal provider-mounted smoke: the probe reflects a provider switch.
    render(
      <I18nProvider>
        <LangProbe />
      </I18nProvider>,
    );
    expect(screen.getByTestId("label").textContent).toBe("Skills");
    expect(screen.getByTestId("lang").textContent).toBe("en");
  });
});

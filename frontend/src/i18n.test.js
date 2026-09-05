import {
  COPY,
  GATE_COPY,
  LANGUAGE_STORAGE_KEY,
  copyFor,
  loadLanguagePreference,
  normalizeLanguage,
  stateLabel,
  storeLanguagePreference,
} from "./i18n";

test("French is the safe default and English is selectable", () => {
  expect(copyFor()).toBe(COPY.fr);
  expect(copyFor("en").hero.titleLead).toBe("Extend the useful life");
  expect(copyFor("fr").hero.titleLead).toBe("Prolongez la durée de vie");
});

test("runtime labels translate presentation without changing canonical values", () => {
  expect(stateLabel("REMOTE_SUCCESS", "fr")).toBe("Succès distant");
  expect(stateLabel("REMOTE_SUCCESS", "en")).toBe("Remote success");
  expect(stateLabel("LOCAL_FALLBACK_AFTER_REMOTE_UNAVAILABLE", "en")).toBe(
    "Local fallback after remote unavailable"
  );
  expect(stateLabel("LOCAL_FALLBACK_AFTER_AMBIGUOUS_REMOTE", "fr")).toBe(
    "Fallback local après résultat distant ambigu"
  );
  expect(stateLabel("FRESH_HINT", "en")).toBe("Fresh hint");
  expect(stateLabel("FRESH_PASS", "fr")).toBe("Dernière preuve réussie");
  expect(stateLabel("STALE", "en")).toBe("Stale");
  expect(stateLabel("UNRECOGNIZED_CANONICAL_STATE", "fr")).toBe("UNRECOGNIZED_CANONICAL_STATE");
});

test("five gate identifiers retain their canonical mapping", () => {
  expect(GATE_COPY.paired.fr).toBe("Appairé");
  expect(GATE_COPY.authenticated.en).toBe("Authenticated");
  expect(GATE_COPY.controller_lease.en).toBe("Controller lease");
  expect(GATE_COPY.resource_admissible.fr).toBe("Dernière preuve d’admission/readiness");
  expect(GATE_COPY.provider_ready.en).toBe("Provider ready");
});

test("language preference accepts only the two supported values", () => {
  expect(normalizeLanguage("fr")).toBe("fr");
  expect(normalizeLanguage("en")).toBe("en");
  expect(normalizeLanguage("EN")).toBe("fr");
  expect(normalizeLanguage("de")).toBe("fr");
  expect(normalizeLanguage(null)).toBe("fr");
});

test("language preference reloads from its dedicated storage key", () => {
  const getItem = jest.fn(() => "en");

  expect(loadLanguagePreference({ localStorage: { getItem } })).toBe("en");
  expect(getItem).toHaveBeenCalledWith(LANGUAGE_STORAGE_KEY);
});

test("invalid or unavailable language storage fails safely to French", () => {
  expect(
    loadLanguagePreference({ localStorage: { getItem: () => "unsupported" } })
  ).toBe("fr");
  expect(
    loadLanguagePreference({
      get localStorage() {
        throw new Error("storage unavailable");
      },
    })
  ).toBe("fr");
  expect(loadLanguagePreference(null)).toBe("fr");
});

test("only a normalized language preference is persisted", () => {
  const setItem = jest.fn();
  const storageWindow = { localStorage: { setItem } };

  expect(storeLanguagePreference(storageWindow, "en")).toBe("en");
  expect(setItem).toHaveBeenLastCalledWith(LANGUAGE_STORAGE_KEY, "en");
  expect(storeLanguagePreference(storageWindow, "unsupported")).toBe("fr");
  expect(setItem).toHaveBeenLastCalledWith(LANGUAGE_STORAGE_KEY, "fr");
});

test("language persistence remains usable when writes are blocked", () => {
  const blocked = {
    localStorage: {
      setItem() {
        throw new Error("storage unavailable");
      },
    },
  };

  expect(storeLanguagePreference(blocked, "en")).toBe("en");
});

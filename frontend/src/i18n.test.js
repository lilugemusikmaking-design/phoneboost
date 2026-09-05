import { COPY, GATE_COPY, copyFor, stateLabel } from "./i18n";

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
  expect(stateLabel("UNRECOGNIZED_CANONICAL_STATE", "fr")).toBe("UNRECOGNIZED_CANONICAL_STATE");
});

test("five gate identifiers retain their canonical mapping", () => {
  expect(GATE_COPY.paired.fr).toBe("Appairé");
  expect(GATE_COPY.authenticated.en).toBe("Authenticated");
  expect(GATE_COPY.controller_lease.en).toBe("Controller lease");
  expect(GATE_COPY.resource_admissible.fr).toBe("Ressource admissible");
  expect(GATE_COPY.provider_ready.en).toBe("Provider ready");
});

import { act } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import { LANGUAGE_STORAGE_KEY } from "./i18n";

jest.mock("@/pages/Dashboard", () => {
  const React = require("react");
  return {
    __esModule: true,
    default: ({ language, setLanguage }) =>
      React.createElement(
        "button",
        {
          "data-testid": "language-toggle",
          onClick: () => setLanguage(language === "fr" ? "en" : "fr"),
        },
        language
      ),
  };
});

global.IS_REACT_ACT_ENVIRONMENT = true;

function mountApp() {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = createRoot(container);
  act(() => root.render(<App />));
  return {
    container,
    unmount() {
      act(() => root.unmount());
      container.remove();
    },
  };
}

beforeEach(() => {
  window.localStorage.clear();
  document.documentElement.lang = "";
});

afterEach(() => {
  document.body.innerHTML = "";
});

test("App restores the persisted English preference on startup", () => {
  window.localStorage.setItem(LANGUAGE_STORAGE_KEY, "en");
  const view = mountApp();

  expect(view.container.textContent).toBe("en");
  expect(document.documentElement.lang).toBe("en");
  expect(window.localStorage.getItem(LANGUAGE_STORAGE_KEY)).toBe("en");
  view.unmount();
});

test("App persists a language change without storing other state", () => {
  const view = mountApp();
  const toggle = view.container.querySelector('[data-testid="language-toggle"]');

  act(() => toggle.click());

  expect(view.container.textContent).toBe("en");
  expect(document.documentElement.lang).toBe("en");
  expect(window.localStorage.getItem(LANGUAGE_STORAGE_KEY)).toBe("en");
  expect(window.localStorage.length).toBe(1);
  view.unmount();
});

test("App replaces an invalid persisted value with the safe French default", () => {
  window.localStorage.setItem(LANGUAGE_STORAGE_KEY, "unsupported");
  const view = mountApp();

  expect(view.container.textContent).toBe("fr");
  expect(document.documentElement.lang).toBe("fr");
  expect(window.localStorage.getItem(LANGUAGE_STORAGE_KEY)).toBe("fr");
  view.unmount();
});

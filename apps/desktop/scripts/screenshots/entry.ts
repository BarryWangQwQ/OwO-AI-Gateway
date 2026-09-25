// Screenshot entry: a clean, dark, fixed-language start (`?lang=zh-CN|en|ja`), no motion, then the real app.
const lang = new URLSearchParams(location.search).get("lang") ?? "zh-CN";

localStorage.clear();
localStorage.setItem("owo-lang", lang);
localStorage.setItem("owo-theme", "dark");
document.documentElement.classList.add("dark");

const still = document.createElement("style");
still.textContent = `*, *::before, *::after {
  animation-duration: 0s !important;
  animation-delay: 0s !important;
  transition-duration: 0s !important;
  transition-delay: 0s !important;
  caret-color: transparent !important;
}`;
document.head.append(still);

await import("../../src/main.tsx");

export {};

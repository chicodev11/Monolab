const REPO = "chicodev11/Monolab";
const LANGUAGES = ["en"];
const tables = {};
let words = {};

function pickLanguage() {
  try {
    const saved = localStorage.getItem("monolab-site-lang");
    if (LANGUAGES.includes(saved)) return saved;
  } catch {}
  const wanted = navigator.languages?.length ? navigator.languages : [navigator.language];
  for (const one of wanted) {
    const code = String(one ?? "").toLowerCase().split("-")[0];
    if (LANGUAGES.includes(code)) return code;
  }
  return "en";
}

function table(lang) {
  tables[lang] ??= fetch(`/lang/${lang}.json`).then((answer) => (answer.ok ? answer.json() : {})).catch(() => ({}));
  return tables[lang];
}

function t(text, args = []) {
  const found = words[text] ?? text;
  return found.replace(/\{(\d+)\}/g, (_, n) => String(args[Number(n)] ?? ""));
}

async function setLanguage(lang) {
  words = await table(lang);
  document.documentElement.lang = lang;
  try {
    localStorage.setItem("monolab-site-lang", lang);
  } catch {}
  document.querySelectorAll("[data-t]").forEach((one) => {
    one.dataset.source ??= one.innerHTML;
    one.innerHTML = t(one.dataset.source);
  });
  document.querySelectorAll(".lang button").forEach((one) => one.classList.toggle("on", one.dataset.set === lang));
  document.dispatchEvent(new CustomEvent("language"));
}

for (const button of document.querySelectorAll(".lang button")) {
  button.addEventListener("click", () => setLanguage(button.dataset.set));
  table(button.dataset.set).then((found) => (button.textContent = found.$language ?? button.dataset.set));
}
setLanguage(pickLanguage());

// The repo also has add-on (addon-<n>), fork (fork-<n>) and "updates" releases, so pick the newest
// app release (tag v<x.y.z>) that has an installer instead of trusting releases/latest.
async function wireDownload() {
  const links = document.querySelectorAll(".get");
  if (!links.length) return;
  try {
    const answer = await fetch(`https://api.github.com/repos/${REPO}/releases?per_page=30`);
    if (!answer.ok) return;
    const releases = await answer.json();
    const release = releases.find(
      (one) => !one.draft && !one.prerelease && /^v\d/.test(one.tag_name) && one.assets.some((asset) => asset.name.endsWith("-setup.exe")),
    );
    if (!release) return;
    const setup = release.assets.find((one) => one.name.endsWith("-setup.exe"));
    links.forEach((link) => {
      link.href = setup.browser_download_url;
      link.setAttribute("download", "");
    });
    document.querySelectorAll(".version").forEach((one) => (one.textContent = release.tag_name.replace(/^v/, "")));
  } catch {}
}
wireDownload();

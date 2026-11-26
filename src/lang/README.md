# Translations

Every language file of the app is here:

- `en.json` — English. Screens use `tr("English sentence")`, so the sentence itself is the key; this file only holds
  named keys like `card.start`.
- `<code>.json` (like `ko.json`) — "English sentence": "translation", in sections. Sections are only for people;
  the app reads every section the same way.
- `rust.<code>.json` — messages that come from the app's Rust side (errors, notices). `{name}` in a key matches
  anything; `$1`, `$2` in the translation put those parts back.

Working on a language:

1. `npm run i18n:todo -- ko` — puts every sentence `ko.json` is missing into its `"todo"` section with an empty
   translation, and moves sentences the app no longer uses to `"unused"`. Run it again after updating the app.
2. Fill in `"todo"` (by hand or with an AI). An empty translation shows in English. Keep `{0}`, `{name}`,
   `{0, plural, …}` and the like as they are.
3. `"ready": false` at the top keeps the language out of the Language setting while it's being done (`npm run dev`
   shows it anyway). Set it to `true` or delete it to ship it.

`npm run i18n:check` must report 0 missing for `en.json`; other languages only show how far they got.
